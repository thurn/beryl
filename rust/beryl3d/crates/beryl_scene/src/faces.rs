//! The faces a match can show and the texture-array layers they have landed in (docs/beryl-3d.md,
//! "Card faces"). The client makes the [`FaceList`] when the match loads, from the card store's
//! records of every card and token the match can show. A face's index in the list is its
//! [`FaceId`]. The host reads the list (`Client::faces`) so that its asset workers prepare each
//! face, and each worker reports the layer a face landed in as an `AssetReady::Face`.

use std::collections::BTreeMap;

use beryl_session::card_records::{CardEntry, CardRecords};

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
}

/// Every face of a match, in [`FaceId`] order: the card back, then each card's faces, then
/// each token's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FaceList {
    faces: Vec<ListedFace>,
    by_name: BTreeMap<String, FaceId>,
    by_image: BTreeMap<String, FaceId>,
}

impl FaceList {
    /// The faces of the cards and tokens in `records`. A card's first face is also found by the
    /// card's name and by the name it was asked for.
    #[must_use]
    pub fn from_records(records: &CardRecords) -> Self {
        let mut list = Self::default();
        list.add(&records.card_back, Vec::new(), &[]);
        for (asked, entry) in records.cards.iter().chain(&records.tokens) {
            list.add_entry(asked, entry);
        }
        list
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

    fn add_entry(&mut self, asked: &str, entry: &CardEntry) {
        for (i, face) in entry.faces.iter().enumerate() {
            let mut names = vec![face.name.clone()];
            if i == 0 {
                names.extend([entry.name.clone(), asked.to_owned()]);
            }
            self.add(&face.images.large, names, &face.colors);
        }
    }

    /// Adds the face showing `image`, or adds `names` to the face already listed with it. A
    /// name already listed keeps its first face.
    fn add(&mut self, image: &str, names: Vec<String>, colors: &[String]) {
        let next = FaceId(u16::try_from(self.faces.len()).expect("a match has under 2^16 faces"));
        let id = *self.by_image.entry(image.to_owned()).or_insert(next);
        if id == next {
            self.faces.push(ListedFace {
                image: image.to_owned(),
                names: Vec::new(),
                colors: colors.to_vec(),
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

/// The client's faces: the match's list, and the layer each face has landed in at each size.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Faces {
    list: FaceList,
    /// By [`FaceId`]: the face's layer in the board-size array, once it has one.
    board: Vec<Option<u16>>,
    /// By [`FaceId`]: the face's layer in the full-size array, once it has one.
    full: Vec<Option<u16>>,
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
        }
    }

    #[must_use]
    pub fn list(&self) -> &FaceList {
        &self.list
    }

    /// Records that `face` has landed in `layer` of the array for `size`. A face the list does
    /// not hold, from a match that has since been replaced, is ignored.
    pub fn arrived(&mut self, face: FaceId, size: FaceSize, layer: u16) {
        let layers = match size {
            FaceSize::Board => &mut self.board,
            FaceSize::Full => &mut self.full,
        };
        if let Some(slot) = layers.get_mut(usize::from(face.0)) {
            *slot = Some(layer);
        }
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

    /// The full-size layer of `face`, once it has one.
    #[must_use]
    pub fn full_layer_of(&self, face: FaceId) -> Option<u16> {
        self.full.get(usize::from(face.0)).copied().flatten()
    }
}
