//! The fonts bundled in the client core (docs/beryl-3d.md, "Determinism": no system fonts).
//! Text is laid out with these files alone, so a line breaks in the same place on every machine.
//! The three text families are variable fonts, from which parley selects any weight; each file's
//! licence lies beside it in `fonts/`.

/// One bundled font file.
#[derive(Clone, Copy, Debug)]
pub struct FontFile {
    pub font: Font,
    /// The file's name in `fonts/`.
    pub file: &'static str,
    pub data: &'static [u8],
}

/// A bundled font, by its index in [`ALL`]. A [`GlyphKey`](crate::input::GlyphKey) names its
/// font by this index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Font {
    /// Cinzel, the display face.
    Cinzel,
    /// Crimson Pro, for card names, rules text and prompts.
    CrimsonPro,
    /// Open Sans, for interface text.
    OpenSans,
    /// The mana symbol font.
    Mana,
}

impl Font {
    /// The font's family name as the web interface's CSS names it, without the `Variable`
    /// that its variable fonts' names end in.
    #[must_use]
    pub fn family(self) -> &'static str {
        match self {
            Self::Cinzel => "Cinzel",
            Self::CrimsonPro => "Crimson Pro",
            Self::OpenSans => "Open Sans",
            Self::Mana => "Mana",
        }
    }

    /// The font's file.
    #[must_use]
    pub fn file(self) -> &'static FontFile {
        &ALL[self as usize]
    }
}

/// Every bundled font file, in the order of [`Font`], for the glyph rasterizer.
pub static ALL: [FontFile; 4] = [
    FontFile {
        font: Font::Cinzel,
        file: "Cinzel.ttf",
        data: include_bytes!("../../fonts/Cinzel.ttf"),
    },
    FontFile {
        font: Font::CrimsonPro,
        file: "CrimsonPro.ttf",
        data: include_bytes!("../../fonts/CrimsonPro.ttf"),
    },
    FontFile {
        font: Font::OpenSans,
        file: "OpenSans.ttf",
        data: include_bytes!("../../fonts/OpenSans.ttf"),
    },
    FontFile {
        font: Font::Mana,
        file: "mana.ttf",
        data: include_bytes!("../../fonts/mana.ttf"),
    },
];
