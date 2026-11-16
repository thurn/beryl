//! The faces a match can show and the texture-array layers they have landed in (docs/beryl-3d.md,
//! "Card faces"). The client makes the [`FaceList`] when the match loads, from the card store's
//! records of every card and token the match can show. A face's index in the list is its
//! [`FaceId`]. The host reads the list (`Client::faces`) so that its asset workers prepare each
//! face, and each worker reports the layer a face landed in as an `AssetReady::Face`.

use std::collections::BTreeMap;

use beryl_protocol::card_records::{CardEntry, CardRecords};

use crate::input::{AssetReady, FaceId, FaceSize};

/// The card back's [`FaceId`]: always the first face.
pub const CARD_BACK: FaceId = FaceId(0);

/// One distinct picture of the match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedFace {
    /// The picture's full-size scan on Scryfall, which also identifies it: two names that show
    /// the same scan share one face.
    pub image: String,
    /// The card and face names that show this picture; the first is drawn on a placeholder.
    /// Empty for the card back.
    pub names: Vec<String>,
    /// The face's colors as the card store gives them (`W`, `U`, `B`, `R`, `G`).
    pub colors: Vec<String>,
    /// The power and toughness printed on the face, when both are numbers: a battlefield
    /// plaque inks a value raised or lowered against them.
    pub printed: Option<[i32; 2]>,
    /// What Beryl's drawn frame prints on the face when its scan has not been downloaded.
    pub text: PrintedText,
    /// For a face that shows one ability on the stack, which ability: its source's picture
    /// wearing a plate that says the ability. `None` for a card's own face.
    pub plate: Option<Plate>,
}

/// The words a face prints, as the card store gives them, which Beryl's drawn frame sets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrintedText {
    /// The mana cost in `{...}` notation, such as `{1}{U}{U}`.
    pub mana_cost: Option<String>,
    pub type_line: String,
    /// The rules text, one ability to a line, with symbols in `{...}` notation.
    pub rules: Option<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
    /// The face is a token's, which the frame draws in pewter where a card's is black.
    pub token: bool,
}

/// One ability of a face, said on a plate over the face's lower half, as an ability on the
/// stack shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plate {
    /// The face whose picture the plate covers.
    pub source: FaceId,
    /// A triggered ability rather than an activated one, which the plate's label says.
    pub triggered: bool,
    /// The paragraph of the source's rules text that says the ability.
    pub rules: String,
}

/// Every face of a match, in [`FaceId`] order: the card back, then each card's faces, then
/// each token's, then a plate face for each ability paragraph of those faces' rules text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FaceList {
    faces: Vec<ListedFace>,
    by_name: BTreeMap<String, FaceId>,
    by_image: BTreeMap<String, FaceId>,
    /// The plate face of each ability, by its source's face, whether it is triggered, and how
    /// many paragraphs of that kind come before it (`StackItem::Ability`'s `nth`).
    plates: BTreeMap<(FaceId, bool, u8), FaceId>,
}

impl FaceList {
    /// The faces of the cards and tokens in `records`. A card's first face is also found by the
    /// card's name and by the name it was asked for.
    #[must_use]
    pub fn from_records(records: &CardRecords) -> Self {
        let mut list = Self::default();
        list.add(
            &records.card_back,
            Vec::new(),
            &[],
            None,
            PrintedText::default(),
        );
        for (asked, entry) in &records.cards {
            list.add_entry(asked, entry, false);
        }
        for (asked, entry) in &records.tokens {
            list.add_entry(asked, entry, true);
        }
        list.add_plates();
        list
    }

    /// Lists a plate face for each paragraph of each face's rules text that says a triggered
    /// or an activated ability.
    pub(crate) fn add_plates(&mut self) {
        let sources = self.faces.len();
        for at in 0..sources {
            let source = FaceId(u16::try_from(at).expect("a match has under 2^16 faces"));
            let Some(rules) = self.faces[at].text.rules.clone() else {
                continue;
            };
            for triggered in [true, false] {
                let paragraphs = rules.lines().filter(|p| says_ability(p, triggered));
                for (nth, paragraph) in paragraphs.enumerate() {
                    let Ok(nth) = u8::try_from(nth) else {
                        break;
                    };
                    let id = FaceId(
                        u16::try_from(self.faces.len()).expect("a match has under 2^16 faces"),
                    );
                    let face = &self.faces[at];
                    self.faces.push(ListedFace {
                        image: face.image.clone(),
                        names: Vec::new(),
                        colors: face.colors.clone(),
                        printed: None,
                        text: face.text.clone(),
                        plate: Some(Plate {
                            source,
                            triggered,
                            rules: paragraph.to_owned(),
                        }),
                    });
                    self.plates.insert((source, triggered, nth), id);
                }
            }
        }
    }

    /// The plate face of the ability of the face named `name` that is the `nth` triggered (or
    /// activated) ability its rules text says, as `StackItem::Ability` names it. Allocates
    /// nothing.
    #[must_use]
    pub fn plate(&self, name: &str, triggered: bool, nth: u8) -> Option<FaceId> {
        let source = self.find(name)?;
        self.plates.get(&(source, triggered, nth)).copied()
    }

    /// The faces, in [`FaceId`] order.
    #[must_use]
    pub fn faces(&self) -> &[ListedFace] {
        &self.faces
    }

    /// Each face ready at board size in the layer numbered as its [`FaceId`], as `beryl_assets`
    /// lays them out: what a host whose asset workers have every face ready gives the client.
    pub fn all_ready(&self) -> impl Iterator<Item = AssetReady> + '_ {
        (0..self.faces.len()).map(|at| {
            let face = FaceId(u16::try_from(at).expect("a match has under 2^16 faces"));
            AssetReady::Face {
                face,
                size: FaceSize::Board,
                layer: face.0,
            }
        })
    }

    /// The face with this id.
    #[must_use]
    pub fn get(&self, face: FaceId) -> Option<&ListedFace> {
        self.faces.get(usize::from(face.0))
    }

    /// The face that shows the card or face named `name`. Allocates nothing.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<FaceId> {
        self.by_name.get(name).copied()
    }

    fn add_entry(&mut self, asked: &str, entry: &CardEntry, token: bool) {
        for (i, face) in entry.faces.iter().enumerate() {
            let mut names = vec![face.name.clone()];
            if i == 0 {
                names.extend([entry.name.clone(), asked.to_owned()]);
            }
            let printed = face
                .power
                .as_deref()
                .zip(face.toughness.as_deref())
                .and_then(|(power, toughness)| {
                    Some([power.parse().ok()?, toughness.parse().ok()?])
                });
            let text = PrintedText {
                mana_cost: face.mana_cost.clone(),
                type_line: face.type_line.clone(),
                rules: face.oracle_text.clone(),
                power: face.power.clone(),
                toughness: face.toughness.clone(),
                loyalty: face.loyalty.clone(),
                token,
            };
            self.add(&face.images.large, names, &face.colors, printed, text);
        }
    }

    /// Adds a sample for the component gallery. Its image key has no Scryfall scan, so the
    /// asset worker draws the frame from the supplied text and colors.
    pub(crate) fn add_preview(&mut self, name: &str, colors: &str, text: PrintedText) {
        self.add(
            &format!("gallery/{name}"),
            vec![name.to_owned()],
            &colors
                .chars()
                .map(|color| color.to_string())
                .collect::<Vec<_>>(),
            None,
            text,
        );
    }

    /// Adds the face showing `image`, or adds `names` to the face already listed with it. A
    /// name already listed keeps its first face.
    fn add(
        &mut self,
        image: &str,
        names: Vec<String>,
        colors: &[String],
        printed: Option<[i32; 2]>,
        text: PrintedText,
    ) {
        let next = FaceId(u16::try_from(self.faces.len()).expect("a match has under 2^16 faces"));
        let id = *self.by_image.entry(image.to_owned()).or_insert(next);
        if id == next {
            self.faces.push(ListedFace {
                image: image.to_owned(),
                names: Vec::new(),
                colors: colors.to_vec(),
                printed,
                text,
                plate: None,
            });
        }
        let listed = &mut self.faces[usize::from(id.0)];
        for name in names {
            if !listed.names.contains(&name) {
                listed.names.push(name.clone());
            }
            self.by_name.entry(name).or_insert(id);
        }
    }
}

/// Whether `paragraph` of a face's rules text says a triggered ability (it begins "When",
/// "Whenever" or "At", or its reminder text says one, as offspring's does) or an activated one
/// (a cost and a colon before any reminder text, loyalty abilities included). The web
/// interface finds an ability's paragraph by the same rule (`abilityParagraph` in
/// `ui/src/game/faces.ts`), and the session counts paragraphs as both do.
#[must_use]
pub fn says_ability(paragraph: &str, triggered: bool) -> bool {
    if !triggered {
        let before_reminder = paragraph.split('(').next().unwrap_or("");
        return before_reminder.contains(':');
    }
    let begins = ["When", "Whenever", "At"].iter().any(|word| {
        paragraph
            .strip_prefix(word)
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
    });
    let reminder = paragraph.find('(').is_some_and(|open| {
        let inside = &paragraph[open..];
        ["when", "When", "whenever", "Whenever"]
            .iter()
            .any(|word| has_word(inside, word))
            || inside.contains("At the beginning")
    });
    begins || reminder
}

/// Whether `word` appears in `text` with no letter, digit or underscore on either side.
fn has_word(text: &str, word: &str) -> bool {
    let joins = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back().is_none_or(|c| !joins(c));
        let after = text[at + word.len()..]
            .chars()
            .next()
            .is_none_or(|c| !joins(c));
        before && after
    })
}

/// The client's faces: the match's list, and the layer each face has landed in at each size.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Faces {
    list: FaceList,
    /// By [`FaceId`]: the face's layer in the board-size array, once it has one.
    board: Vec<Option<u16>>,
    /// By [`FaceId`]: the face's layer in the full-size array, once it has one.
    full: Vec<Option<u16>>,
    /// The faces asked for at full size (`want_full`) that the host has not been told of yet.
    wanted: Vec<FaceId>,
    /// By [`FaceId`]: whether the face has been asked for at full size since it last lost its
    /// full-size layer, so that it is asked for once.
    asked: Vec<bool>,
}

impl Faces {
    /// A new match's faces, none of which has a layer yet.
    #[must_use]
    pub fn new(list: FaceList) -> Self {
        let count = list.faces.len();
        Self {
            list,
            board: vec![None; count],
            full: vec![None; count],
            wanted: Vec::new(),
            asked: vec![false; count],
        }
    }

    #[must_use]
    pub fn list(&self) -> &FaceList {
        &self.list
    }

    /// Records that `face` has landed in `layer` of the array for `size`. A face the list does
    /// not hold, from a match that has since been replaced, is ignored. The full-size array
    /// holds few layers and reuses the least recently asked-for one, so a full-size face that
    /// held `layer` before has lost it, and is drawn at board size until it is asked for again.
    pub fn arrived(&mut self, face: FaceId, size: FaceSize, layer: u16) {
        if size == FaceSize::Full && usize::from(face.0) < self.full.len() {
            for (held, asked) in self.full.iter_mut().zip(&mut self.asked) {
                if *held == Some(layer) {
                    *held = None;
                    *asked = false;
                }
            }
        }
        let layers = match size {
            FaceSize::Board => &mut self.board,
            FaceSize::Full => &mut self.full,
        };
        if let Some(slot) = layers.get_mut(usize::from(face.0)) {
            *slot = Some(layer);
        }
    }

    /// Forgets every face's full-size layer, because the renderer freed the full-size array when
    /// memory ran low (docs/beryl-3d.md, "Hosts": "Low memory"). Those faces draw from their
    /// board-size layers until a full-size layer arrives again. Board-size layers are kept.
    pub fn low_memory(&mut self) {
        self.full.fill(None);
        self.asked.fill(false);
    }

    /// Asks for `face` at full size, because a card is being read (docs/beryl-3d.md, "Sizes
    /// and memory"). A face that has a full-size layer, or has been asked for already, is not
    /// asked for again. Until its full-size layer arrives it is drawn from its board-size one.
    pub fn want_full(&mut self, face: FaceId) {
        let at = usize::from(face.0);
        if self.full_layer_of(face).is_some() || self.asked.get(at).is_none_or(|&asked| asked) {
            return;
        }
        self.asked[at] = true;
        self.wanted.push(face);
    }

    /// The faces asked for at full size since the last call, each once, for the host to hand
    /// its asset workers.
    pub fn drain_wanted(&mut self) -> impl Iterator<Item = FaceId> + '_ {
        self.wanted.drain(..)
    }

    /// The face that shows the card or face named `name`. Allocates nothing.
    #[must_use]
    pub fn face(&self, name: &str) -> Option<&ListedFace> {
        self.list.get(self.list.find(name)?)
    }

    /// The board-size layer of the face that shows the card or face named `name`, once it has
    /// one. Allocates nothing.
    #[must_use]
    pub fn board_layer(&self, name: &str) -> Option<u16> {
        self.board_layer_of(self.list.find(name)?)
    }

    /// The board-size layer of `face`, once it has one.
    #[must_use]
    pub fn board_layer_of(&self, face: FaceId) -> Option<u16> {
        self.board.get(usize::from(face.0)).copied().flatten()
    }

    /// The board-size layer of the plate face of an ability on the stack (`FaceList::plate`),
    /// once it has one. Allocates nothing.
    #[must_use]
    pub fn plate_layer(&self, name: &str, triggered: bool, nth: u8) -> Option<u16> {
        self.board_layer_of(self.list.plate(name, triggered, nth)?)
    }

    /// The full-size layer of `face`, once it has one.
    #[must_use]
    pub fn full_layer_of(&self, face: FaceId) -> Option<u16> {
        self.full.get(usize::from(face.0)).copied().flatten()
    }
}
