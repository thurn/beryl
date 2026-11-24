//! The desktop's clipboard, through arboard: a field's copied or cut selection goes on it
//! (`Request::Copy`), and a paste reads it (`Request::Paste`). The host's event loop calls it
//! while it carries out the client's requests, after a frame, so no frame waits for it.

/// The system clipboard, opened at its first use.
#[derive(Default)]
pub struct Clipboard {
    opened: Option<arboard::Clipboard>,
}

impl Clipboard {
    /// Puts `text` on the clipboard. A clipboard that cannot be opened or written leaves it as
    /// it was, and says so on standard error.
    pub fn copy(&mut self, text: String) {
        if let Some(clipboard) = self.open()
            && let Err(error) = clipboard.set_text(text)
        {
            eprintln!("the clipboard could not be written: {error}");
        }
    }

    /// The clipboard's text, or `None` when it holds none or cannot be read.
    pub fn paste(&mut self) -> Option<String> {
        self.open()?.get_text().ok()
    }

    fn open(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.opened.is_none() {
            match arboard::Clipboard::new() {
                Ok(clipboard) => self.opened = Some(clipboard),
                Err(error) => eprintln!("the clipboard could not be opened: {error}"),
            }
        }
        self.opened.as_mut()
    }
}
