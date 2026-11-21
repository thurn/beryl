//! The faces a match can show and the texture-array layers they have landed in (docs/beryl-3d.md,
//! "Card faces"). The client makes the [`FaceList`] when the match loads, from the card store's
//! records of every card and token the match can show. A face's index in the list is its
//! [`FaceId`]. The host reads the list (`Client::faces`) so that its asset workers prepare each
//! face, and each worker reports the layer a face landed in as an `AssetReady::Face`.

use std::collections::BTreeMap;

use beryl_protocol::card_records::{CardEntry, CardRecords};
use beryl_protocol::protocol::{ObjectView, PlayerView, StackItem, ZoneName};

use kit_ui::input::{AssetReady, FaceId, FaceSize};

/// The card back's [`FaceId`]: always the first face.
pub const CARD_BACK: FaceId = FaceId(0);

/// The most faces the stack and the hands ask for at full size (`FaceList::read_whole`). The
/// full-size array holds 16 layers, and the rest stay free for the cards being read otherwise:
/// the card under the pointer, the inspector and a prompt's cards.
pub const READ_WHOLE: usize = 10;

/// The faces `FaceList::read_whole` lists, in a fixed array so that listing them allocates
/// nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadWhole {
    faces: [FaceId; READ_WHOLE + 1],
    len: usize,
}

impl ReadWhole {
    /// The faces, the card back first.
    #[must_use]
    pub fn faces(&self) -> &[FaceId] {
        &self.faces[..self.len]
    }
}

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
    /// Each face by every name it shows, with the name's hash (`name_hash`), sorted by hash and
    /// then by name. Every card drawn looks its face up here on every frame, and comparing one
    /// hash at each step of the search costs a fraction of comparing names along a tree's path.
    by_name: Vec<(u64, String, FaceId)>,
    by_image: BTreeMap<String, FaceId>,
    /// The plate face of each ability, by its source's face, whether it is triggered, and how
    /// many paragraphs of that kind come before it (`StackItem::Ability`'s `nth`).
    plates: BTreeMap<(FaceId, bool, u8), FaceId>,
    /// Each card's faces in its record's order, by the card's name and the name it was asked
    /// for.
    cards: BTreeMap<String, Vec<FaceId>>,
    /// Each card's mana value (`mana_value`), by the card's name and the name it was asked
    /// for. It is worked out from the card's record, because the halves of a split card share
    /// one scan and so one listed face.
    mana_values: BTreeMap<String, u32>,
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

    /// The faces of the cards `view` shows where the web interface draws a card's full frame
    /// rather than its battlefield frame (`variant` in `ui/src/components/Card.tsx`): the
    /// spells and abilities on the stack, top first, then the face-up cards in the hands, after
    /// the card back when there are any. Each face is listed once, and at most [`READ_WHOLE`]
    /// besides the card back are. The client asks for them at full size (`Faces::want_full`),
    /// so that these cards show their cost, type line and rules text. Allocates nothing: the
    /// client lists them whenever it rebuilds its screen.
    #[must_use]
    pub fn read_whole(&self, view: &PlayerView) -> ReadWhole {
        let mut faces = ReadWhole {
            faces: [CARD_BACK; READ_WHOLE + 1],
            len: 1,
        };
        let mut add = |face: Option<FaceId>| {
            if let Some(face) = face
                && faces.len <= READ_WHOLE
                && !faces.faces().contains(&face)
            {
                faces.faces[faces.len] = face;
                faces.len += 1;
            }
        };
        for item in view.stack.iter().rev() {
            match item {
                StackItem::Spell { slot, .. } => {
                    let object = view.objects.iter().find(|object| object.slot == *slot);
                    add(object.and_then(|object| self.find_object(object)));
                }
                StackItem::Ability {
                    name,
                    triggered,
                    nth,
                    text,
                    ..
                } => add(if text.is_none() {
                    self.plate(name, *triggered, *nth)
                } else {
                    self.find(name)
                }),
            }
        }
        for object in view.objects.iter().filter(|o| o.zone == ZoneName::Hand) {
            add(self.find_object(object));
        }
        // A card drawn from the full-size array takes its back from there too, so the card
        // back leads the list when there are any others.
        if faces.len == 1 {
            faces.len = 0;
        }
        faces
    }

    /// The face that shows `object`, from its token printing first, which tells apart tokens
    /// that share a name, and then from its name. `None` for a card whose name the viewer
    /// does not know.
    #[must_use]
    pub fn find_object(&self, object: &ObjectView) -> Option<FaceId> {
        (object.token_printing.as_deref())
            .and_then(|printing| self.find(printing))
            .or_else(|| self.find(object.name.as_deref()?))
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
        let hash = name_hash(name);
        let at = self
            .by_name
            .binary_search_by(|(other_hash, other, _)| {
                (*other_hash, other.as_str()).cmp(&(hash, name))
            })
            .ok()?;
        Some(self.by_name[at].2)
    }

    /// The face of the card named `card` that is its `index`th, counting from 0 in its
    /// record's order: the face a spell is cast with (`Candidate::Cast`'s `face`). Allocates
    /// nothing.
    #[must_use]
    pub fn face_of(&self, card: &str, index: u8) -> Option<FaceId> {
        self.cards.get(card)?.get(usize::from(index)).copied()
    }

    /// The mana value of the card named `card` outside the stack, as the web's `manaValue`
    /// (`ui/src/table/TableViewer.tsx`) works it out from its printed cost: a split card adds
    /// up both halves' costs and any other card counts its first face's. X counts as 0, a
    /// hybrid symbol with a number as that number, and any other symbol as 1. `None` for a card
    /// not in the match.
    #[must_use]
    pub fn mana_value(&self, card: &str) -> Option<u32> {
        self.mana_values.get(card).copied()
    }

    /// The type line of the first face of the card named `card`, if it is in the match.
    #[must_use]
    pub fn type_line(&self, card: &str) -> Option<&str> {
        let face = self.cards.get(card)?.first()?;
        Some(&self.faces[usize::from(face.0)].text.type_line)
    }

    /// The face the card named `card` is played with as a land: its first face whose type
    /// line says Land, as the web's `landFace` (`ui/src/cards.ts`) finds it ([CR 712.9]):
    /// "A player playing a modal double-faced card or a copy of a modal double-faced card as a
    /// land chooses one of its faces that’s a land before putting it onto the battlefield."
    /// Allocates nothing.
    ///
    /// [CR 712.9]: https://yawgatog.com/resources/magic-rules/#R7129
    #[must_use]
    pub fn land_face(&self, card: &str) -> Option<FaceId> {
        (self.cards.get(card)?.iter().copied()).find(|&face| {
            (self.faces[usize::from(face.0)].text.type_line)
                .split(|c: char| !c.is_alphanumeric())
                .any(|word| word == "Land")
        })
    }

    fn add_entry(&mut self, asked: &str, entry: &CardEntry, token: bool) {
        let counted = if entry.layout == "split" {
            entry.faces.len()
        } else {
            entry.faces.len().min(1)
        };
        let mana_value = (entry.faces[..counted].iter())
            .map(|face| cost_value(face.mana_cost.as_deref().unwrap_or("")))
            .sum();
        for name in [asked, entry.name.as_str()] {
            self.mana_values
                .entry(name.to_owned())
                .or_insert(mana_value);
        }
        let mut ids = Vec::with_capacity(entry.faces.len());
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
            ids.push(self.add(&face.images.large, names, &face.colors, printed, text));
        }
        if !token {
            for name in [asked, entry.name.as_str()] {
                self.cards
                    .entry(name.to_owned())
                    .or_insert_with(|| ids.clone());
            }
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
    ) -> FaceId {
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
            let hash = name_hash(&name);
            let search = self.by_name.binary_search_by(|(other_hash, other, _)| {
                (*other_hash, other.as_str()).cmp(&(hash, &name))
            });
            if let Err(at) = search {
                self.by_name.insert(at, (hash, name, id));
            }
        }
        id
    }
}

/// A hash of a name in the manner of `FxHash`, which the Rust compiler uses for its own tables: it
/// mixes in eight bytes at a time with a rotation and a multiplication. Equal names hash alike;
/// different names rarely do.
pub(crate) fn name_hash(name: &str) -> u64 {
    let mut hash: u64 = 0;
    let mut mix = |word: u64| {
        hash = (hash.rotate_left(5) ^ word).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    };
    mix(name.len() as u64);
    let (words, rest) = name.as_bytes().as_chunks::<8>();
    for &word in words {
        mix(u64::from_le_bytes(word));
    }
    if !rest.is_empty() {
        let mut word = [0; 8];
        word[..rest.len()].copy_from_slice(rest);
        mix(u64::from_le_bytes(word));
    }
    hash
}

/// What the mana symbols of `cost`, written as `{1}{U}{U}`, add up to (`FaceList::mana_value`).
fn cost_value(cost: &str) -> u32 {
    (cost.split('{').skip(1))
        .filter_map(|part| part.split_once('}'))
        .map(|(symbol, _)| match symbol {
            "X" | "Y" | "Z" => 0,
            _ => (symbol.split('/').next())
                .and_then(|generic| generic.parse().ok())
                .unwrap_or(1),
        })
        .sum()
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
    /// How many lists came before this one (`replace`), so that a host can tell a new list from
    /// an equal one given again, whose faces have no layers again either.
    number: u64,
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
            // Room for what the hands and the stack ask for at once (`FaceList::read_whole`)
            // and a few more, so that asking for them while a match plays allocates nothing.
            wanted: Vec::with_capacity(2 * (READ_WHOLE + 1)),
            asked: vec![false; count],
            number: 0,
        }
    }

    /// Replaces the list with a new match's, none of whose faces has a layer yet, and numbers
    /// it one more than the list it replaces.
    pub fn replace(&mut self, list: FaceList) {
        let number = self.number + 1;
        *self = Self {
            number,
            ..Self::new(list)
        };
    }

    /// How many lists came before this one. A host makes its face arrays again whenever it
    /// changes, even for a list equal to the last.
    #[must_use]
    pub fn number(&self) -> u64 {
        self.number
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

    /// The listed face and its board-size layer, if resident, from one name lookup.
    pub(crate) fn listed_board(&self, name: &str) -> (Option<&ListedFace>, Option<u16>) {
        self.list.find(name).map_or((None, None), |id| {
            (self.list.get(id), self.board_layer_of(id))
        })
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

    /// The full-size layers of `face` and of the card back, once both have them: a card drawn
    /// from the full-size array takes its back from that array too.
    #[must_use]
    pub fn full_layers(&self, face: Option<FaceId>) -> Option<[u16; 2]> {
        Some([self.full_layer_of(face?)?, self.full_layer_of(CARD_BACK)?])
    }

    /// The full-size layers of the face that shows `object` (`FaceList::find_object`) and of
    /// the card back, once both have them.
    #[must_use]
    pub fn full_layers_of_object(&self, object: &ObjectView) -> Option<[u16; 2]> {
        self.full_layers(self.list.find_object(object))
    }

    /// The full-size layers of the plate face of an ability on the stack (`FaceList::plate`) and
    /// of the card back, once both have them.
    #[must_use]
    pub fn plate_full_layers(&self, name: &str, triggered: bool, nth: u8) -> Option<[u16; 2]> {
        self.full_layers(self.list.plate(name, triggered, nth))
    }

    /// The full-size layers of the face that shows the card or face named `name` and of the
    /// card back, once both have them.
    #[must_use]
    pub fn full_layers_named(&self, name: &str) -> Option<[u16; 2]> {
        self.full_layers(self.list.find(name))
    }
}
