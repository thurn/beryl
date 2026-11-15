//! The list of every face a match can show, made when the match loads (docs/beryl-3d.md, "Card
//! faces", "Preparation"): the card back, then each distinct picture of both decklists' cards and
//! of the tokens they make. A face's index in the list is its [`FaceId`] and its layer in the
//! board-size texture array.

use std::collections::BTreeMap;

use beryl_card_store::Record;
use beryl_scene::input::FaceId;
use beryl_session::card_records::{CARD_BACK_ID, image, record};

/// Scryfall's card image host, which `card_records::image` puts before a face's path.
const IMAGES: &str = "https://cards.scryfall.io";
/// The Scryfall image version both face sizes are made from: 672 by 936 pixels, full size.
const VERSION: &str = "large";

/// One face of the match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchFace {
    /// The scan's path for `beryl_art::ArtCache::request`, such as
    /// `/large/front/a/b/ab12….jpg`, or `/backs/large/0/a/0aee….jpg` for the card back.
    pub scan: String,
    /// The picture's identity in the compressed cache: the printing's Scryfall id and the side.
    pub key: String,
    /// The card and face names that show this picture; the first is drawn on a placeholder.
    pub names: Vec<String>,
    /// The face's colors as the card store gives them (`W`, `U`, `B`, `R`, `G`), which choose a
    /// placeholder's color.
    pub colors: Vec<String>,
}

/// Every face of a match, in [`FaceId`] order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MatchFaces {
    pub faces: Vec<MatchFace>,
    by_name: BTreeMap<String, FaceId>,
    by_key: BTreeMap<String, FaceId>,
}

/// The card back's [`FaceId`]: always the first face.
pub const CARD_BACK: FaceId = FaceId(0);

impl MatchFaces {
    /// The faces of `decklists`, each a list of `(count, card name)`, and of every token their
    /// cards make. A name the card store does not know has no face.
    #[must_use]
    pub fn list(decklists: &[&[(u8, &str)]]) -> Self {
        let mut faces = Self::default();
        let back = &CARD_BACK_ID;
        faces.add(MatchFace {
            scan: format!(
                "/backs/{VERSION}/{}/{}/{back}.jpg",
                &back[0..1],
                &back[1..2]
            ),
            key: format!("back-{back}"),
            names: Vec::new(),
            colors: Vec::new(),
        });
        let mut tokens = Vec::new();
        for &(_, name) in decklists.iter().flat_map(|list| list.iter()) {
            if let Some(card) = record(name, false) {
                tokens.extend(card.tokens.iter().map(|token| token.name.clone()));
                faces.add_record(&card, name);
            }
        }
        for name in tokens {
            if let Some(token) = record(&name, true) {
                faces.add_record(&token, &name);
            }
        }
        faces
    }

    /// The face that shows the card or face named `name`.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<FaceId> {
        self.by_name.get(name).copied()
    }

    /// The face with this id.
    #[must_use]
    pub fn get(&self, face: FaceId) -> Option<&MatchFace> {
        self.faces.get(usize::from(face.0))
    }

    fn add_record(&mut self, card: &Record, asked: &str) {
        let id = &card.scryfall_id;
        for (i, face) in card.faces.iter().enumerate() {
            let side = face.side.as_str();
            let url = image(VERSION, side, id);
            let mut names = vec![face.name.clone()];
            if i == 0 {
                names.extend([card.name.clone(), asked.to_owned()]);
            }
            self.add(MatchFace {
                scan: url.strip_prefix(IMAGES).unwrap_or(&url).to_owned(),
                key: format!("{id}-{side}"),
                names,
                colors: face.colors.clone(),
            });
        }
    }

    /// Adds `face`, or its names to the face already listed with the same picture.
    fn add(&mut self, face: MatchFace) {
        let next = FaceId(self.faces.len() as u16);
        let id = *self.by_key.entry(face.key.clone()).or_insert(next);
        if id == next {
            self.faces.push(MatchFace {
                names: Vec::new(),
                ..face.clone()
            });
        }
        let at = usize::from(id.0);
        for name in face.names {
            self.by_name.entry(name.clone()).or_insert(id);
            if !self.faces[at].names.contains(&name) {
                self.faces[at].names.push(name);
            }
        }
    }
}
