//! The plate faces: an ability on the stack, or one offered in a prompt, is drawn as its
//! source's picture wearing a plate that says the ability.
//!
//! A card in a deck with Secret Lair printings (`data/deck-printings.csv`) shows one of them
//! (`ObjectView::printing`), so its abilities wear their plates over that printing's picture:
//! a Dandân drawn as the koi printing puts its trigger on the stack with the koi's art, not the
//! card's ordinary art.

use beryl_protocol::card_records::PrintingIx;
use kit_ui::input::FaceId;

use super::{FaceList, ListedFace, Plate, says_ability};

impl FaceList {
    /// Lists a plate face for each paragraph of each face's rules text that says a triggered
    /// or an activated ability.
    pub fn add_plates(&mut self) {
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
                    let face = &self.faces[at];
                    let plate = Plate {
                        source,
                        triggered,
                        choice: false,
                        rules: paragraph.to_owned(),
                    };
                    self.push_plate(face.clone(), plate, nth);
                }
            }
        }
    }

    /// Lists a plate face for each plate of the ordinary face of each printing's card, over the
    /// printing's picture. The plates copy the ordinary face's paragraphs, so a printing's
    /// abilities are numbered (`StackItem::Ability`'s `nth`) as the card's are even where the
    /// printing's own text is worded or broken into lines differently.
    pub(super) fn add_printing_plates(&mut self) {
        let printings: Vec<(PrintingIx, FaceId)> = self
            .printings
            .iter()
            .map(|(&ix, &face)| (ix, face))
            .collect();
        for (_, printed) in printings {
            let Some(name) = self.faces[usize::from(printed.0)].names.first() else {
                continue;
            };
            let Some(ordinary) = self.find(name) else {
                continue;
            };
            let plates: Vec<((bool, u8), FaceId)> = (self.plates.iter())
                .filter(|&(&(source, ..), _)| source == ordinary)
                .map(|(&(_, triggered, nth), &face)| ((triggered, nth), face))
                .collect();
            for ((_, nth), plate) in plates {
                let Some(plate) = self.faces[usize::from(plate.0)].plate.clone() else {
                    continue;
                };
                let face = self.faces[usize::from(printed.0)].clone();
                let plate = Plate {
                    source: printed,
                    ..plate
                };
                self.push_plate(face, plate, nth);
            }
        }
    }

    /// Lists `plate` over the picture of `face` as the `nth` ability of its kind of the plate's
    /// source, with a second, centered plate for the mode chooser when it is an activated one.
    fn push_plate(&mut self, face: ListedFace, plate: Plate, nth: u8) {
        let id = FaceId(u16::try_from(self.faces.len()).expect("a match has under 2^16 faces"));
        let (source, triggered) = (plate.source, plate.triggered);
        let ability = ListedFace {
            image: face.image,
            names: Vec::new(),
            colors: face.colors,
            printed: None,
            text: face.text,
            plate: Some(plate),
        };
        if triggered {
            self.faces.push(ability);
        } else {
            let mut choice = ability.clone();
            choice
                .plate
                .as_mut()
                .expect("the ability has a plate")
                .choice = true;
            let choice_id = FaceId(id.0.checked_add(1).expect("a match has under 2^16 faces"));
            self.choice_plates.insert((source, nth), choice_id);
            self.faces.extend([ability, choice]);
        }
        self.plates.insert((source, triggered, nth), id);
    }

    /// The face whose picture the abilities of the card named `name` wear: the face of
    /// `printing`, the printing its source object shows, when the match lists it, and else the
    /// card's ordinary face. Allocates nothing.
    #[must_use]
    pub fn ability_source(&self, name: &str, printing: Option<PrintingIx>) -> Option<FaceId> {
        (printing.and_then(|ix| self.printing(ix))).or_else(|| self.find(name))
    }

    /// The plate face of the ability of the face named `name`, shown as `printing`, that is the
    /// `nth` triggered (or activated) ability its rules text says, as `StackItem::Ability`
    /// names it. Allocates nothing.
    #[must_use]
    pub fn plate(
        &self,
        name: &str,
        printing: Option<PrintingIx>,
        triggered: bool,
        nth: u8,
    ) -> Option<FaceId> {
        let source = self.ability_source(name, printing)?;
        self.plates.get(&(source, triggered, nth)).copied()
    }

    /// The face an ability on the stack is drawn with, as `StackItem::Ability` names it, where
    /// `printing` is the printing its source object shows (`PlayerView::ability_printing`):
    /// its plate face, when its words are a paragraph of its source's printed text (it has no
    /// `text` of its own) and the source has a plate for it, and otherwise the face of its
    /// source. A delayed trigger that a loyalty ability's paragraph creates has no `text` and
    /// no plate. The table draws the ability with this face and the client asks for the same
    /// face at full size (`read_whole`), so the two cannot disagree. Allocates nothing.
    #[must_use]
    pub fn ability_face(
        &self,
        name: &str,
        printing: Option<PrintingIx>,
        triggered: bool,
        nth: u8,
        text: Option<&str>,
    ) -> Option<FaceId> {
        let source = self.ability_source(name, printing)?;
        if let Some(rules) = text {
            return self
                .find_custom_plate(source, triggered, rules)
                .or(Some(source));
        }
        self.plates
            .get(&(source, triggered, nth))
            .copied()
            .or(Some(source))
    }

    /// The centered plate of an activated ability read in the mode chooser, over the picture
    /// of `printing` when its source shows one.
    #[must_use]
    pub fn choice_plate(
        &self,
        name: &str,
        printing: Option<PrintingIx>,
        nth: u8,
    ) -> Option<FaceId> {
        let source = self.ability_source(name, printing)?;
        self.choice_plates.get(&(source, nth)).copied()
    }
}
