//! Writes the frame log off the main thread and marks the complete prefix a bug can attach.

use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};

use crate::frame_log::Line;

#[expect(
    clippy::large_enum_variant,
    reason = "keep log lines inline so adding the bug prefix request does not box each line"
)]
pub(crate) enum Message {
    Line(Line),
    /// Asks for the file's length once every line sent before it is written: the prefix a bug
    /// report attaches, or proof that the log is on disk before the process ends.
    Written(Sender<u64>),
}

pub struct Attachment {
    path: PathBuf,
    complete: Receiver<u64>,
}

impl Attachment {
    pub(crate) fn new(path: &Path, complete: Receiver<u64>) -> Self {
        Self {
            path: path.to_owned(),
            complete,
        }
    }

    pub(crate) fn read(self) -> io::Result<Vec<u8>> {
        // Only the report worker waits. It reads through the writer's last complete line,
        // even if another capture is in progress by the time it opens the file.
        let length = self.complete.recv().map_err(io::Error::other)?;
        let mut bytes = Vec::new();
        File::open(self.path)?
            .take(length)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

pub(crate) fn run(
    dir: &Path,
    path: &Path,
    first: &Line,
    received: Receiver<Message>,
) -> io::Result<()> {
    beryl_host::utility_thread();
    fs::create_dir_all(dir)?;
    let mut file = BufWriter::new(File::create(path)?);
    write(&mut file, first)?;
    for message in received {
        match message {
            Message::Line(line) => write(&mut file, &line)?,
            Message::Written(complete) => {
                let _ = complete.send(file.get_ref().metadata()?.len());
            }
        }
    }
    Ok(())
}

fn write(file: &mut BufWriter<File>, line: &Line) -> io::Result<()> {
    serde_json::to_writer(&mut *file, line)?;
    file.write_all(b"\n")?;
    file.flush()
}
