//! The fonts embedded in the client core (docs/beryl-3d.md, "Determinism": no system fonts at
//! run time). Text is laid out with these files alone, so a line breaks in the same place on
//! every run.
//!
//! Cinzel, Crimson Pro and Open Sans are variable fonts committed in `fonts/`, from which
//! parley selects any weight; each file's licence lies beside it. The web interface's serif
//! face, `--font-serif`, is Times New Roman, which no licence lets the repository hold. The
//! build script embeds the installed copy on macOS and Windows, with Crimson Pro as
//! the fallback ([`Font::Serif`]).

include!(concat!(env!("OUT_DIR"), "/serif.rs"));

/// One embedded font file.
#[derive(Clone, Copy, Debug)]
pub struct FontFile {
    pub font: Font,
    /// The file's name: in `fonts/`, or in the system's font folder for the serif faces.
    pub file: &'static str,
    pub data: &'static [u8],
}

/// An embedded font, by the index of its regular face in [`ALL`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Font {
    /// Cinzel, the display face.
    Cinzel,
    /// Crimson Pro, the web's card face (`--font-card`): card names, rules text, life totals
    /// and the numbers on a card's badges.
    CrimsonPro,
    /// Open Sans, for interface text.
    OpenSans,
    /// The font for inline symbols.
    Symbols,
    /// The web's serif face (`--font-serif`), for titles, buttons and captions: Times New
    /// Roman where the build finds it, or Crimson Pro otherwise. Times New Roman
    /// is not variable, so it has a regular, a bold, an italic and a bold italic file, and
    /// [`Font::face`] picks one.
    Serif,
}

/// The index in [`ALL`] of the serif face's bold file, which no [`Font`] names.
const SERIF_BOLD: u8 = 5;

/// The index in [`ALL`] of the serif face's italic file; its bold italic file follows it.
const SERIF_ITALIC: u8 = 6;

impl Font {
    /// The font's family name as the web interface's CSS names it, without the `Variable`
    /// that its variable fonts' names end in. For [`Font::Serif`], the family it is drawn in.
    #[must_use]
    pub fn family(self) -> &'static str {
        match self {
            Self::Cinzel => "Cinzel",
            Self::CrimsonPro => "Crimson Pro",
            Self::OpenSans => "Open Sans",
            Self::Symbols => "Symbols",
            Self::Serif => SERIF_FAMILY,
        }
    }

    /// The font's regular file.
    #[must_use]
    pub fn file(self) -> &'static FontFile {
        &ALL[self as usize]
    }

    /// The index in [`ALL`] of the file that draws the font at CSS weight `weight`, italic when
    /// `italic` is set (CSS `font-style: italic`). Only the serif face has more than one file:
    /// as CSS matches a weight when only 400 and 700 exist, a weight above 500 is drawn bold
    /// and any other regular. An italic is drawn from an italic file where the serif face has
    /// one ([`SERIF_HAS_ITALIC`]); every other font, and Crimson Pro standing in for Times New
    /// Roman, draws its italics upright.
    #[must_use]
    pub fn face(self, weight: u16, italic: bool) -> u8 {
        if self != Self::Serif {
            return self as u8;
        }
        let bold = weight > 500;
        match (italic && SERIF_HAS_ITALIC, bold) {
            (false, false) => self as u8,
            (false, true) => SERIF_BOLD,
            (true, false) => SERIF_ITALIC,
            (true, true) => SERIF_ITALIC + 1,
        }
    }
}

/// Crimson Pro's bytes, which the serif faces share where the build found no Times New Roman.
static CRIMSON_PRO: &[u8] = include_bytes!("../../fonts/CrimsonPro.ttf");

/// Every embedded font file: each [`Font`]'s regular file in the order of [`Font`], then the
/// serif face's bold, italic and bold italic. A [`GlyphKey`](crate::atlas::GlyphKey) names its
/// file by its index here.
pub static ALL: [FontFile; 8] = [
    FontFile {
        font: Font::Cinzel,
        file: "Cinzel.ttf",
        data: include_bytes!("../../fonts/Cinzel.ttf"),
    },
    FontFile {
        font: Font::CrimsonPro,
        file: "CrimsonPro.ttf",
        data: CRIMSON_PRO,
    },
    FontFile {
        font: Font::OpenSans,
        file: "OpenSans.ttf",
        data: include_bytes!("../../fonts/OpenSans.ttf"),
    },
    FontFile {
        font: Font::Symbols,
        file: "symbols.ttf",
        data: include_bytes!("../../fonts/symbols.ttf"),
    },
    FontFile {
        font: Font::Serif,
        file: SERIF_REGULAR_FILE,
        data: SERIF_REGULAR,
    },
    FontFile {
        font: Font::Serif,
        file: SERIF_BOLD_FILE,
        data: SERIF_BOLD_DATA,
    },
    FontFile {
        font: Font::Serif,
        file: SERIF_ITALIC_FILE,
        data: SERIF_ITALIC_DATA,
    },
    FontFile {
        font: Font::Serif,
        file: SERIF_BOLD_ITALIC_FILE,
        data: SERIF_BOLD_ITALIC_DATA,
    },
];

/// Registers each file of [`ALL`] once through `register`, which returns the family name the
/// file's font has, and returns the family name of every file of [`ALL`] in its order. A file
/// whose bytes an earlier one already holds (the serif faces, when they fall back to Crimson
/// Pro) is not registered again, and takes the earlier file's family.
pub fn register_all(mut register: impl FnMut(&'static FontFile) -> String) -> Vec<String> {
    let mut families: Vec<String> = Vec::with_capacity(ALL.len());
    for (index, file) in ALL.iter().enumerate() {
        let earlier = ALL[..index]
            .iter()
            .position(|other| std::ptr::eq(other.data.as_ptr(), file.data.as_ptr()));
        let family = match earlier {
            Some(earlier) => families[earlier].clone(),
            None => register(file),
        };
        families.push(family);
    }
    families
}
