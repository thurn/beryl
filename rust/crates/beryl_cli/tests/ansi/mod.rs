//! A small ANSI terminal for tests: replays the bytes a program wrote and keeps the screen
//! they leave, scrollback included. It knows only what the long-think panel writes: text,
//! carriage return, newline (as a terminal's output processing makes it, a carriage return
//! too), cursor up (`ESC[nA`), clear line (`ESC[2K`), cursor show and hide (`ESC[?25h`,
//! `ESC[?25l`) and colors (`ESC[...m`, ignored). Anything else fails the test, as does a line
//! written past the width.

pub struct Term {
    width: usize,
    rows: Vec<Vec<char>>,
    pub row: usize,
    col: usize,
    pub cursor_shown: bool,
}

impl Term {
    /// The screen after `bytes`, on a terminal `width` columns wide.
    pub fn replay(bytes: &[u8], width: usize) -> Term {
        let mut t = Term {
            width,
            rows: vec![Vec::new()],
            row: 0,
            col: 0,
            cursor_shown: true,
        };
        let text = String::from_utf8_lossy(bytes);
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\r' => t.col = 0,
                '\n' => {
                    t.row += 1;
                    t.col = 0;
                    if t.row == t.rows.len() {
                        t.rows.push(Vec::new());
                    }
                }
                '\x1b' => {
                    assert_eq!(chars.next(), Some('['), "an escape that is no CSI");
                    let mut params = String::new();
                    let end = loop {
                        let c = chars.next().expect("an unterminated escape");
                        if c.is_ascii_alphabetic() {
                            break c;
                        }
                        params.push(c);
                    };
                    match (end, params.as_str()) {
                        ('A', n) => {
                            let n: usize = if n.is_empty() { 1 } else { n.parse().unwrap() };
                            t.row = t.row.checked_sub(n).expect("the cursor above the top");
                        }
                        ('K', "2") => t.rows[t.row].clear(),
                        ('l', "?25") => t.cursor_shown = false,
                        ('h', "?25") => t.cursor_shown = true,
                        ('m', _) => {}
                        (end, params) => panic!("an unknown escape: ESC[{params}{end}"),
                    }
                }
                ch => {
                    assert!(t.col < t.width, "a line past {} columns", t.width);
                    let row = &mut t.rows[t.row];
                    if row.len() <= t.col {
                        row.resize(t.col + 1, ' ');
                    }
                    row[t.col] = ch;
                    t.col += 1;
                }
            }
        }
        t
    }

    /// The screen's lines, right-trimmed, without the empty ones at its end.
    pub fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .rows
            .iter()
            .map(|r| r.iter().collect::<String>().trim_end().to_owned())
            .collect();
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines
    }
}
