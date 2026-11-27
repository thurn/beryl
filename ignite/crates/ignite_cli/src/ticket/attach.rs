//! `ignite ticket attach`, `ignite ticket attachment get|list` and `ignite ticket create
//! --attach`: image attachments on tickets (see `ignite_core::protocol::attachments`).
//!
//! An image comes from a file, from the clipboard on macOS, or (`--pasted`) from the transcript
//! of the Claude Code session running the command. A screenshot a person pastes into a session
//! exists nowhere else on disk: Claude Code keeps it as a base64 `image` block in the user's
//! message in `~/.claude/projects/<project folder>/<session>.jsonl`. The client checks each
//! image's type and size before sending it, as the hub does again.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde_json::{Value, json};

use ignite_core::protocol::attachments::{
    Attachment, MAX_ATTACHMENT_BYTES, embed, size_text, sniff_attachment,
};
use ignite_core::protocol::client::{Exit, Reply};

use super::http::{Hub, Result, usage};
use super::{build, ids, write};

#[derive(Debug, Args)]
pub struct AttachArgs {
    pub id: String,
    /// Files to attach: PNG, JPEG, GIF, WebP, JSON, JSON Lines, plain text or gzip, at most 20 MiB
    /// each.
    pub files: Vec<PathBuf>,
    /// The Nth most recent image pasted into this Claude Code session; 1 when N is left out.
    #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "1")]
    pub pasted: Option<usize>,
    /// The image on the macOS clipboard.
    #[arg(long)]
    pub clipboard: bool,
    /// The name to store the image under, for a single image; the hub adds `-2`, `-3` to a
    /// name the ticket already uses.
    #[arg(long)]
    pub name: Option<String>,
    /// A caption, shown under the image and used as its alt text.
    #[arg(long)]
    pub caption: Option<String>,
    /// Add no comment embedding the images in the ticket's timeline.
    #[arg(long)]
    pub no_comment: bool,
}

#[derive(Debug, Subcommand)]
pub enum AttachmentCommand {
    /// Download an attachment and print the path it was saved to.
    Get {
        id: String,
        name: String,
        /// Where to save it; `$TMPDIR/ignite-attachments/<id>/<name>` by default.
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
    /// List a ticket's attachments.
    List { id: String },
}

/// One image to upload: the name to ask for and its bytes.
struct Image {
    name: String,
    bytes: Vec<u8>,
}

/// `ignite ticket attach`.
pub fn attach(hub: &Hub, args: &AttachArgs) -> Result<Reply> {
    let env = |name: &str| std::env::var(name).ok();
    let mut images = Vec::new();
    for file in &args.files {
        let bytes = std::fs::read(file).map_err(|e| usage(format!("{}: {e}", file.display())))?;
        let name = file
            .file_name()
            .map_or_else(|| "image".to_owned(), |n| n.to_string_lossy().into_owned());
        images.push(Image { name, bytes });
    }
    if let Some(n) = args.pasted {
        images.push(Image {
            name: "pasted".to_owned(),
            bytes: pasted(n, &env)?,
        });
    }
    if args.clipboard {
        images.push(Image {
            name: "clipboard".to_owned(),
            bytes: clipboard()?,
        });
    }
    if images.is_empty() {
        return Err(usage(
            "name an image file, or pass --pasted for an image pasted into this session or \
             --clipboard",
        ));
    }
    if args.name.is_some() && images.len() > 1 {
        return Err(usage(
            "--name names a single image; attach the others separately",
        ));
    }
    if let Some(name) = &args.name {
        name.clone_into(&mut images[0].name);
    }
    let stored = upload_all(hub, &args.id, &images, args.caption.as_deref())?;
    let comment = !args.no_comment;
    if comment {
        let text: Vec<String> = stored
            .iter()
            .map(|a| embed(&a.name, a.caption.as_deref()))
            .collect();
        write(
            hub,
            "commented on",
            build::single(json!({"op": "comment", "id": args.id, "text": text.join("\n")})),
        )?;
    }
    let mut value = json!({ "id": args.id, "attachments": stored, "commented": comment });
    ids::display(&mut value);
    let mut text: Vec<String> = stored
        .iter()
        .map(|a| {
            format!(
                "attached  {} {}",
                ids::spelled(a.ticket.as_str()),
                a.summary()
            )
        })
        .collect();
    if comment {
        text.push(format!(
            "commented on {} with the images",
            ids::spelled(&args.id)
        ));
    }
    Ok(Reply {
        exit: Exit::Success,
        text: text.join("\n"),
        json: Some(value),
        handle: None,
    })
}

/// `create --attach`: uploads `files` to the ticket `created` says was filed, and adds them to
/// its reply.
pub fn after_create(hub: &Hub, mut created: Reply, files: &[PathBuf]) -> Result<Reply> {
    if files.is_empty() {
        return Ok(created);
    }
    let id = created
        .json
        .as_ref()
        .and_then(|reply| reply["results"][0]["id"].as_str())
        .ok_or_else(|| usage("the hub's reply names no created ticket"))?
        .to_owned();
    let stored = attach_files(hub, &id, files)?;
    for attachment in &stored {
        created.text.push_str("\nattached  ");
        created.text.push_str(&id);
        created.text.push(' ');
        created.text.push_str(&attachment.summary());
    }
    if let Some(reply) = created.json.as_mut() {
        reply["attachments"] = json!(stored);
    }
    Ok(created)
}

/// Uploads `files` to the ticket `ticket`.
fn attach_files(hub: &Hub, ticket: &str, files: &[PathBuf]) -> Result<Vec<Attachment>> {
    let mut images = Vec::new();
    for file in files {
        let bytes = std::fs::read(file).map_err(|e| usage(format!("{}: {e}", file.display())))?;
        let name = file
            .file_name()
            .map_or_else(|| "image".to_owned(), |n| n.to_string_lossy().into_owned());
        images.push(Image { name, bytes });
    }
    upload_all(hub, ticket, &images, None)
}

/// Checks every image before sending any, then uploads each.
fn upload_all(
    hub: &Hub,
    ticket: &str,
    images: &[Image],
    caption: Option<&str>,
) -> Result<Vec<Attachment>> {
    let mut types = Vec::new();
    for image in images {
        types.push(check(&image.name, &image.bytes)?);
    }
    let mut stored = Vec::new();
    for (image, media_type) in images.iter().zip(types) {
        let mut query = vec![("name", image.name.clone())];
        query.extend(caption.map(|caption| ("caption", caption.to_owned())));
        let reply = hub.post_bytes(
            &["tickets", ticket, "attachments"],
            &query,
            &image.bytes,
            media_type,
        )?;
        stored.push(serde_json::from_str(&reply.body).map_err(usage)?);
    }
    Ok(stored)
}

/// The file's media type, or why the hub would refuse it.
fn check(name: &str, bytes: &[u8]) -> Result<&'static str> {
    if bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
        return Err(usage(format!(
            "{name} is {}, over the limit of {}",
            size_text(bytes.len() as u64),
            size_text(MAX_ATTACHMENT_BYTES)
        )));
    }
    sniff_attachment(name, bytes)
        .map(|file| file.media_type)
        .ok_or_else(|| {
            usage(format!(
                "{name} is not a PNG, JPEG, GIF, WebP, JSON, JSON Lines, plain text or gzip file"
            ))
        })
}

/// The `n`th most recent image pasted into the Claude Code session `CLAUDE_CODE_SESSION_ID`.
fn pasted(n: usize, env: &dyn Fn(&str) -> Option<String>) -> Result<Vec<u8>> {
    const OTHERWISE: &str = "save the image and pass its path, or use --clipboard";
    let session = env("CLAUDE_CODE_SESSION_ID")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            usage(format!(
                "--pasted reads the transcript of the Claude Code session running it, and \
             CLAUDE_CODE_SESSION_ID is not set; {OTHERWISE}"
            ))
        })?;
    let config = env("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|home| Path::new(&home).join(".claude")))
        .ok_or_else(|| usage("HOME is not set"))?;
    let transcript = find_transcript(&config.join("projects"), &session).ok_or_else(|| {
        usage(format!(
            "no transcript of session {session} under {}; {OTHERWISE}",
            config.join("projects").display()
        ))
    })?;
    let text = std::fs::read_to_string(&transcript)
        .map_err(|e| usage(format!("{}: {e}", transcript.display())))?;
    let mut images = images_in_transcript(&text);
    let count = images.len();
    if n == 0 || n > count {
        return Err(usage(format!(
            "--pasted={n} asks for the image pasted {n} back, and this session's transcript \
             holds {count}; {OTHERWISE}"
        )));
    }
    Ok(images.swap_remove(count - n))
}

/// The transcript `<session>.jsonl` in any project folder under `projects`.
pub(crate) fn find_transcript(projects: &Path, session: &str) -> Option<PathBuf> {
    let file = format!("{session}.jsonl");
    std::fs::read_dir(projects)
        .ok()?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path().join(&file))
        .find(|path| path.is_file())
}

/// Every image the person pasted, oldest first: the base64 `image` blocks of the user messages.
/// An image inside a tool result, such as a file the agent read, is not one the person pasted.
pub(crate) fn images_in_transcript(text: &str) -> Vec<Vec<u8>> {
    let mut images = Vec::new();
    for line in text.lines() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if entry["type"] != "user" {
            continue;
        }
        for block in entry["message"]["content"].as_array().into_iter().flatten() {
            if block["type"] == "image"
                && block["source"]["type"] == "base64"
                && let Some(bytes) = block["source"]["data"].as_str().and_then(base64)
            {
                images.push(bytes);
            }
        }
    }
    images
}

/// Decodes standard base64, ignoring whitespace; `None` for anything malformed.
fn base64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(u32::from(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        }))
    };
    let clean: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let data = clean
        .strip_suffix(b"==")
        .or_else(|| clean.strip_suffix(b"="))
        .unwrap_or(&clean);
    let mut out = Vec::with_capacity(data.len() * 3 / 4);
    for chunk in data.chunks(4) {
        if chunk.len() == 1 {
            return None;
        }
        let mut bits = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            bits |= value(c)? << (18 - 6 * i);
        }
        let bytes = bits.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    Some(out)
}

/// The PNG on the macOS clipboard, which `osascript` prints as `«data PNGf<hex>»`.
fn clipboard() -> Result<Vec<u8>> {
    if !cfg!(target_os = "macos") {
        return Err(usage(
            "--clipboard works on macOS only; save the image and pass its path",
        ));
    }
    let output = std::process::Command::new("osascript")
        .args(["-e", "the clipboard as «class PNGf»"])
        .output()
        .map_err(|e| usage(format!("running osascript: {e}")))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let hex = text
        .trim()
        .strip_prefix("«data PNGf")
        .and_then(|rest| rest.strip_suffix('»'))
        .filter(|_| output.status.success())
        .ok_or_else(|| usage("the clipboard holds no image"))?;
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            hex.get(i..i + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        })
        .collect::<Option<Vec<u8>>>()
        .ok_or_else(|| usage("the clipboard's image could not be read"))
}

/// `ignite ticket attachment get` and `list`.
pub fn attachment(hub: &Hub, command: &AttachmentCommand) -> Result<Reply> {
    match command {
        AttachmentCommand::List { id } => {
            let detail = super::read(hub, &["tickets", id], &[])?;
            let list: Vec<Attachment> =
                serde_json::from_value(detail["attachments"].clone()).unwrap_or_default();
            let shown = ids::spelled(detail["id"].as_str().unwrap_or(id));
            let text = if list.is_empty() {
                format!("{shown} has no attachments")
            } else {
                lines(&shown, &list).join("\n")
            };
            let mut value = json!({ "id": detail["id"], "attachments": list });
            ids::display(&mut value);
            Ok(Reply {
                exit: Exit::Success,
                text,
                json: Some(value),
                handle: None,
            })
        }
        AttachmentCommand::Get { id, name, output } => {
            let bytes = hub.get_bytes(&["tickets", id, "attachments", name], &[])?;
            let path = output.clone().unwrap_or_else(|| default_path(id, name));
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| usage(format!("{}: {e}", parent.display())))?;
            }
            std::fs::write(&path, &bytes).map_err(|e| usage(format!("{}: {e}", path.display())))?;
            Ok(Reply {
                exit: Exit::Success,
                text: path.display().to_string(),
                json: Some(json!({ "path": path, "bytes": bytes.len() })),
                handle: None,
            })
        }
    }
}

/// `$TMPDIR/ignite-attachments/<id>/<name>`, where an agent's file-reading tool can open it.
fn default_path(id: &str, name: &str) -> PathBuf {
    // A download a person or agent asked for goes to the system's temporary directory.
    #[allow(clippy::disallowed_methods)]
    let temporary = std::env::temp_dir();
    let name = Path::new(name)
        .file_name()
        .map_or_else(|| "attachment".into(), std::borrow::ToOwned::to_owned);
    temporary.join("ignite-attachments").join(id).join(name)
}

/// One line per attachment, with the command that fetches each file still kept.
pub fn lines(ticket: &str, list: &[Attachment]) -> Vec<String> {
    list.iter()
        .map(|attachment| {
            if attachment.removed_at.is_some() {
                format!("  {}", attachment.summary())
            } else {
                format!(
                    "  {}  (ignite ticket attachment get {ticket} {})",
                    attachment.summary(),
                    attachment.name
                )
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_decodes_with_and_without_padding() {
        assert_eq!(base64("R0lGODlh").unwrap(), b"GIF89a");
        assert_eq!(base64("aGk=").unwrap(), b"hi");
        assert_eq!(base64("aA==").unwrap(), b"h");
        assert_eq!(base64("aGVs\nbG8=").unwrap(), b"hello");
        assert_eq!(base64("a$=="), None);
    }

    /// A transcript holding two pasted images, a text message and a tool result carrying an
    /// image the agent read, which is not one the person pasted.
    fn transcript() -> String {
        let pasted = |data: &str| {
            json!({"type": "user", "message": {"role": "user", "content": [
                {"type": "text", "text": "look at this"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/gif", "data": data}},
            ]}})
            .to_string()
        };
        let read = json!({"type": "user", "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "t1", "content": [
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw=="}},
            ]},
        ]}});
        let reply =
            json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "ok"}]}});
        [
            pasted("R0lGODdhAQABAA=="),
            reply.to_string(),
            read.to_string(),
            pasted("R0lGODlhAgADAA=="),
        ]
        .join("\n")
    }

    #[test]
    fn pasted_reads_the_nth_most_recent_image_from_the_session_transcript() {
        let home = ignite_core::test_files::temp_dir().join("attach-home");
        let folder = home.join(".claude").join("projects").join("-repo");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("abc.jsonl"), transcript()).unwrap();
        let home_text = home.display().to_string();
        let env = |name: &str| match name {
            "CLAUDE_CODE_SESSION_ID" => Some("abc".to_owned()),
            "HOME" => Some(home_text.clone()),
            _ => None,
        };
        let newest = pasted(1, &env).unwrap();
        assert_eq!(
            sniff_attachment("pasted", &newest).map(|i| (i.width, i.height)),
            Some((2, 3))
        );
        let older = pasted(2, &env).unwrap();
        assert!(older.starts_with(b"GIF87a"));
        let too_far = pasted(3, &env).unwrap_err();
        assert!(too_far.message.contains("holds 2"), "{}", too_far.message);
        let no_session = pasted(1, &|name: &str| (name == "HOME").then(|| home_text.clone()));
        assert!(no_session.unwrap_err().message.contains("--clipboard"));
    }

    #[test]
    fn the_client_refuses_what_the_hub_would() {
        assert_eq!(
            check("a.gif", b"GIF89a\x01\x00\x01\x00").unwrap(),
            "image/gif"
        );
        assert_eq!(
            check("replay.json", br#"{"ok":true}"#).unwrap(),
            "application/json"
        );
        assert_eq!(
            check("journal.jsonl", b"{\"a\":1}\n{\"b\":2}\n").unwrap(),
            "application/x-ndjson"
        );
        assert_eq!(check("tree.txt", b"root\n").unwrap(), "text/plain");
        assert_eq!(
            check("frames.jsonl.gz", &[0x1F, 0x8B, 0x08, 0, 0, 0, 0, 0, 0, 0]).unwrap(),
            "application/gzip"
        );
        assert!(
            check("a.svg", b"<svg/>")
                .unwrap_err()
                .message
                .contains("not a PNG")
        );
        let huge = vec![0u8; usize::try_from(MAX_ATTACHMENT_BYTES).unwrap() + 1];
        assert!(
            check("big.png", &huge)
                .unwrap_err()
                .message
                .contains("over the limit")
        );
    }
}
