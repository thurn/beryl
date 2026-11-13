//! Scryfall's mana cost notation read into counts: what [`crate::mana`] builds a
//! [`crate::ManaCost`] from. It depends on nothing, so `beryl_card_build` includes this file to
//! know which printed costs the engine can hold, as `mana` reads them, with no second list of
//! symbols.

/// A cost's symbols counted: generic mana, the colored and colorless symbols in the order W,
/// U, B, R, G, C, and the {X} symbols.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Notation {
    pub generic: u8,
    pub symbols: [u8; 6],
    pub x: u8,
}

/// Reads `notation`, such as `{1}{U}`: the symbols a cost can hold
/// ([CR 107.4](https://yawgatog.com/resources/magic-rules/#R1074)), the colored and
/// colorless symbols `{W}`, `{U}`, `{B}`, `{R}`, `{G}` and `{C}`, numerical symbols such as
/// `{3}`, and the variable symbol `{X}`.
///
/// # Errors
///
/// What is wrong, on anything else: hybrid, Phyrexian and snow symbols included.
pub const fn read(notation: &str) -> Result<Notation, &'static str> {
    const CANNOT_HOLD: &str = "a mana symbol a cost cannot hold";
    let bytes = notation.as_bytes();
    let mut read = Notation {
        generic: 0,
        symbols: [0; 6],
        x: 0,
    };
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'{' {
            return Err("a mana symbol opens with a brace");
        }
        i += 1;
        let start = i;
        let mut number: u32 = 0;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            number = number * 10 + (bytes[i] - b'0') as u32;
            if number > 255 || read.generic as u32 + number > 255 {
                return Err("more generic mana than a cost holds");
            }
            i += 1;
        }
        if i > start {
            read.generic += number as u8;
        } else {
            if i >= bytes.len() {
                return Err("a mana symbol was left open");
            }
            let index = match bytes[i] {
                b'W' => 0,
                b'U' => 1,
                b'B' => 2,
                b'R' => 3,
                b'G' => 4,
                b'C' => 5,
                b'X' => 6,
                _ => return Err(CANNOT_HOLD),
            };
            if index == 6 {
                read.x += 1;
            } else {
                read.symbols[index] += 1;
            }
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'/' {
            return Err(CANNOT_HOLD);
        }
        if i >= bytes.len() || bytes[i] != b'}' {
            return Err("a mana symbol closes with a brace");
        }
        i += 1;
    }
    Ok(read)
}
