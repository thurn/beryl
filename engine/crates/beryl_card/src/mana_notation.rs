//! Scryfall's mana cost notation read into counts: what [`crate::mana`] builds a
//! [`crate::ManaCost`] from. It depends on nothing, so `beryl_card_build` includes this file to
//! know which printed costs the engine can hold, as `mana` reads them, with no second list of
//! symbols.

/// A cost's symbols counted: generic mana, the colored and colorless symbols in the order W,
/// U, B, R, G, C, the {X} symbols, and the two-color hybrid symbols.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Notation {
    pub generic: u8,
    pub symbols: [u8; 6],
    pub x: u8,
    /// Three bits a pair count the hybrid symbols of each pair, in the order of
    /// [`HYBRID_PAIRS`]: the word `crate::Hybrid` holds.
    pub hybrid: u32,
}

/// The ten two-color hybrid pairs, each as a bit per color in the order W, U, B, R, G: the
/// allied pairs {W/U}, {U/B}, {B/R}, {R/G}, {G/W}, then the enemy pairs {W/B}, {U/R}, {B/G},
/// {R/W}, {G/U}. `crate::Hybrid::PAIRS` is this list.
pub const HYBRID_PAIRS: [u8; 10] = [
    0b00011, 0b00110, 0b01100, 0b11000, 0b10001, 0b00101, 0b01010, 0b10100, 0b01001, 0b10010,
];

/// The most hybrid symbols of one pair a cost holds: three bits of [`Notation::hybrid`].
const MOST_HYBRID: u32 = 7;

/// The bit of a color letter in the order W, U, B, R, G, or none for any other byte.
const fn color_bit(byte: u8) -> Option<u8> {
    match byte {
        b'W' => Some(0b00001),
        b'U' => Some(0b00010),
        b'B' => Some(0b00100),
        b'R' => Some(0b01000),
        b'G' => Some(0b10000),
        _ => None,
    }
}

/// Reads `notation`, such as `{1}{U}`: the symbols a cost can hold
/// ([CR 107.4](https://yawgatog.com/resources/magic-rules/#R1074)), the colored and
/// colorless symbols `{W}`, `{U}`, `{B}`, `{R}`, `{G}` and `{C}`, numerical symbols such as
/// `{3}`, the variable symbol `{X}`, and the two-color hybrid symbols such as `{W/U}`, with
/// their halves in either order.
///
/// # Errors
///
/// What is wrong, on anything else: monocolored hybrid, Phyrexian and snow symbols included.
pub const fn read(notation: &str) -> Result<Notation, &'static str> {
    const CANNOT_HOLD: &str = "a mana symbol a cost cannot hold";
    let bytes = notation.as_bytes();
    let mut read = Notation {
        generic: 0,
        symbols: [0; 6],
        x: 0,
        hybrid: 0,
    };
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'{' {
            return Err("a mana symbol opens with a brace");
        }
        i += 1;
        let start = i;
        let mut number = 0u8;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            let Some(tens) = number.checked_mul(10) else {
                return Err("more generic mana than a cost holds");
            };
            let Some(value) = tens.checked_add(bytes[i] - b'0') else {
                return Err("more generic mana than a cost holds");
            };
            if read.generic.checked_add(value).is_none() {
                return Err("more generic mana than a cost holds");
            }
            number = value;
            i += 1;
        }
        if i > start {
            read.generic += number;
        } else {
            if i >= bytes.len() {
                return Err("a mana symbol was left open");
            }
            if i + 3 < bytes.len() && bytes[i + 1] == b'/' && bytes[i + 3] == b'}' {
                let (Some(a), Some(b)) = (color_bit(bytes[i]), color_bit(bytes[i + 2])) else {
                    return Err(CANNOT_HOLD);
                };
                let mut pair = 0;
                while pair < HYBRID_PAIRS.len() && HYBRID_PAIRS[pair] != a | b {
                    pair += 1;
                }
                if pair == HYBRID_PAIRS.len() {
                    return Err(CANNOT_HOLD);
                }
                let shift = 3 * pair;
                let count = (read.hybrid >> shift) & 0b111;
                if count == MOST_HYBRID {
                    return Err("more hybrid symbols of one pair than a cost holds");
                }
                read.hybrid += 1 << shift;
                i += 4;
                continue;
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
