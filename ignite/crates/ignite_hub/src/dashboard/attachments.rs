//! How a ticket page shows the ticket's image attachments: an image embedded in its text with
//! `![alt](attachment:<name>)` (see [`super::markdown`]), and the "Attachments" section, a grid
//! of thumbnails, so an attachment that no text embeds is still seen.
//!
//! The files are served only on the private listener (`routes::attachments`), because a pasted
//! screenshot can show anything on a person's screen. The public listener's page shows a line
//! in place of each image instead.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use ignite_core::protocol::attachments::{Attachment, size_text};

use super::components::esc;

/// What the public listener shows in place of an image.
pub const PRIVATE_ONLY: &str = "Image, visible on the private dashboard";

/// The address at which the private listener serves a file.
fn file_url(attachment: &Attachment) -> String {
    format!("/attachments/{}", attachment.sha256)
}

/// The linked image, which opens the full file in a new tab.
fn linked_image(attachment: &Attachment, alt: &str) -> String {
    let url = esc(&file_url(attachment));
    format!(
        "<a class=\"c-attachment\" href=\"{url}\" target=\"_blank\" rel=\"noopener\">\
         <img src=\"{url}\" alt=\"{}\" loading=\"lazy\" width=\"{}\" height=\"{}\"></a>",
        esc(alt),
        attachment.width,
        attachment.height
    )
}

/// The HTML for `![alt](attachment:<name>)` in the text of a ticket with `attachments`. An
/// unknown name and a removed file are said in plain text.
pub fn embedded(attachments: &[Attachment], name: &str, alt: &str, private: bool) -> String {
    let Some(attachment) = attachments.iter().find(|a| a.name == name) else {
        return esc(&format!("[missing attachment: {name}]"));
    };
    if let Some(at) = attachment.removed_at {
        return esc(&format!("[image removed {}: {name}]", at.day()));
    }
    if !private {
        return format!("<span class=\"c-muted\">{PRIVATE_ONLY}</span>");
    }
    if attachment.is_image() {
        linked_image(attachment, alt)
    } else {
        let url = esc(&file_url(attachment));
        format!(
            "<a href=\"{url}\" target=\"_blank\" rel=\"noopener\">{}</a>",
            esc(alt)
        )
    }
}

/// The body of the "Attachments" section: one card per attachment with its thumbnail (or a
/// download link for non-image files, or why there is none), name, size and dimensions or type,
/// and caption.
pub fn grid(attachments: &[Attachment], private: bool) -> String {
    let cards: String = attachments
        .iter()
        .map(|attachment| {
            let picture = match (attachment.removed_at, private, attachment.is_image()) {
                (Some(at), _, _) => format!(
                    "<div class=\"c-attachment-gone\">Removed {}</div>",
                    esc(&at.day())
                ),
                (None, false, _) => {
                    format!("<div class=\"c-attachment-gone\">{PRIVATE_ONLY}</div>")
                }
                (None, true, true) => linked_image(
                    attachment,
                    attachment.caption.as_deref().unwrap_or(&attachment.name),
                ),
                (None, true, false) => {
                    let url = esc(&file_url(attachment));
                    format!(
                        "<div class=\"c-attachment-gone\"><a href=\"{url}\" target=\"_blank\" \
                         rel=\"noopener\">{}</a></div>",
                        esc(&attachment.media_type)
                    )
                }
            };
            let meta = if attachment.is_image() {
                format!(
                    "{} · {}×{}",
                    esc(&size_text(attachment.bytes)),
                    attachment.width,
                    attachment.height
                )
            } else {
                format!(
                    "{} · {}",
                    esc(&size_text(attachment.bytes)),
                    esc(&attachment.media_type)
                )
            };
            let caption = attachment
                .caption
                .as_deref()
                .map(|text| format!("<span>{}</span>", esc(text)))
                .unwrap_or_default();
            format!(
                "<figure class=\"c-attachment-card\">{picture}<figcaption>\
                 <span class=\"c-mono\">{}</span>\
                 <span class=\"c-muted\">{meta}</span>{caption}</figcaption></figure>",
                esc(&attachment.name),
            )
        })
        .collect();
    format!("<div class=\"c-attachments\">{cards}</div>")
}
