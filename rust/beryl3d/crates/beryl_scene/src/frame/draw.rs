//! The draw record: one fixed-size record per shape, which the renderer reads in order
//! (docs/beryl-3d.md, "The frame"). What `uv`, `layer` and `params` mean depends on the
//! material, so records are made only by the constructors here, one per material, and each
//! fills only the fields the doc's table gives that material. No code outside this file writes
//! `uv` or `params`.

use glam::{Affine3A, Vec2};

use crate::input::FaceSize;

/// One shape to draw. 80 bytes, laid out as the renderer's instance buffer expects.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Draw {
    /// A 3 by 4 transform into the table's space, row by row: each row is three numbers of the
    /// rotation and scale and then one of the translation.
    pub model: [f32; 12],
    /// By material; see the constructors.
    pub uv: [u16; 4],
    /// Which layer of the texture array, by material.
    pub layer: u16,
    /// A `Material` value.
    pub material: u8,
    /// The texture array (bits 0 to 1), the mesh (bits 2 to 3) and the clip (bits 4 to 7).
    pub flags: u8,
    /// Color and opacity, not premultiplied.
    pub tint: [u8; 4],
    /// By material; see the constructors.
    pub params: [f32; 4],
}

const _: () = assert!(size_of::<Draw>() == 80);

/// One way of shading a shape. Closed: a record's material selects a branch of one shader, and
/// every pipeline is made at startup. A new material is added to the doc's table in the same
/// change.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    Table = 0,
    Card = 1,
    ContactShadow = 2,
    Glow = 3,
    Streak = 4,
    Panel = 5,
    Glyph = 6,
    Icon = 7,
    Particle = 8,
}

/// The texture array a record samples, in bits 0 to 1 of `Draw::flags`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureArray {
    /// The board-size card faces.
    SmallFaces = 0,
    /// The full-size card faces.
    FullFaces = 1,
    /// Everything else: glyph pages, icons, and the shapes of shadows and glows.
    Atlas = 2,
}

/// The mesh a record is drawn with, in bits 2 to 3 of `Draw::flags`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mesh {
    /// A card: a rounded rectangle. The table and a contact shadow use it scaled.
    Card = 0,
    /// The strip around a card's edge.
    CardEdge = 1,
    /// A square from -0.5 to 0.5.
    Quad = 2,
}

/// A rectangle of one atlas page: `rect` is `[x, y, width, height]` in texels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sprite {
    pub page: u16,
    pub rect: [u16; 4],
}

const CLIP_SHIFT: u8 = 4;

impl Draw {
    /// The table. It reads no `uv`, `layer` or `params`.
    #[must_use]
    pub fn table(transform: Affine3A, tint: [u8; 4]) -> Self {
        Self::new(
            transform,
            tint,
            Material::Table,
            TextureArray::SmallFaces,
            Mesh::Card,
        )
    }

    /// A card showing `face` on its front and `back` on its back, both layers of the array for
    /// `size`. The back's layer goes in `uv[0]`, because a face fills its layer and needs no
    /// rectangle. `dissolve` is how far the card has broken up and `whiten` how far its face is
    /// replaced by white light, each from 0 to 1.
    #[must_use]
    pub fn card(
        transform: Affine3A,
        tint: [u8; 4],
        size: FaceSize,
        face: u16,
        back: u16,
        dissolve: f32,
        whiten: f32,
    ) -> Self {
        let array = match size {
            FaceSize::Board => TextureArray::SmallFaces,
            FaceSize::Full => TextureArray::FullFaces,
        };
        let mut draw = Self::new(transform, tint, Material::Card, array, Mesh::Card);
        draw.uv = [back, 0, 0, 0];
        draw.layer = face;
        draw.params = [dissolve, whiten, 0.0, 0.0];
        draw
    }

    /// The soft shadow under a card above the table, blurred by `blur` millimetres.
    #[must_use]
    pub fn contact_shadow(transform: Affine3A, tint: [u8; 4], blur: f32) -> Self {
        let mut draw = Self::new(
            transform,
            tint,
            Material::ContactShadow,
            TextureArray::Atlas,
            Mesh::Card,
        );
        draw.params = [blur, 0.0, 0.0, 0.0];
        draw
    }

    /// A glow `progress` of the way through, from 0 to 1.
    #[must_use]
    pub fn glow(transform: Affine3A, tint: [u8; 4], sprite: Sprite, progress: f32) -> Self {
        let mut draw = Self::sprite(transform, tint, Material::Glow, sprite);
        draw.params = [progress, 0.0, 0.0, 0.0];
        draw
    }

    /// A streak whose head is `head` of the way along its path and whose tail is `tail` of
    /// the path long, each from 0 to 1.
    #[must_use]
    pub fn streak(
        transform: Affine3A,
        tint: [u8; 4],
        sprite: Sprite,
        head: f32,
        tail: f32,
    ) -> Self {
        let mut draw = Self::sprite(transform, tint, Material::Streak, sprite);
        draw.params = [head, tail, 0.0, 0.0];
        draw
    }

    /// A rounded panel of the flat layer, `size` logical points large, with corners of
    /// `radius` and a border `border_width` wide in the color `border` (RGBA, kept in `uv`).
    #[must_use]
    pub fn panel(
        transform: Affine3A,
        tint: [u8; 4],
        border: [u8; 4],
        size: Vec2,
        radius: f32,
        border_width: f32,
    ) -> Self {
        let mut draw = Self::new(
            transform,
            tint,
            Material::Panel,
            TextureArray::Atlas,
            Mesh::Quad,
        );
        draw.uv = border.map(u16::from);
        draw.params = [size.x, size.y, radius, border_width];
        draw
    }

    /// One glyph of laid-out text. It reads no `params`.
    #[must_use]
    pub fn glyph(transform: Affine3A, tint: [u8; 4], sprite: Sprite) -> Self {
        Self::sprite(transform, tint, Material::Glyph, sprite)
    }

    /// An icon. It reads no `params`.
    #[must_use]
    pub fn icon(transform: Affine3A, tint: [u8; 4], sprite: Sprite) -> Self {
        Self::sprite(transform, tint, Material::Icon, sprite)
    }

    /// A particle `age` of the way through its life, from 0 to 1, whose randomness comes from
    /// `seed`.
    #[must_use]
    pub fn particle(
        transform: Affine3A,
        tint: [u8; 4],
        sprite: Sprite,
        age: f32,
        seed: f32,
    ) -> Self {
        let mut draw = Self::sprite(transform, tint, Material::Particle, sprite);
        draw.params = [age, seed, 0.0, 0.0];
        draw
    }

    /// The same record, clipped to entry `clip` (1 to 15) of the frame's clips, or unclipped
    /// for 0.
    #[must_use]
    pub fn with_clip(mut self, clip: u8) -> Self {
        debug_assert!(clip < 16, "a frame has 15 clips");
        self.flags = (self.flags & 0x0f) | (clip << CLIP_SHIFT);
        self
    }

    /// The entry of the frame's clips this record is clipped to, or 0 for none.
    #[must_use]
    pub fn clip(&self) -> u8 {
        self.flags >> CLIP_SHIFT
    }

    fn sprite(transform: Affine3A, tint: [u8; 4], material: Material, sprite: Sprite) -> Self {
        let mut draw = Self::new(transform, tint, material, TextureArray::Atlas, Mesh::Quad);
        draw.uv = sprite.rect;
        draw.layer = sprite.page;
        draw
    }

    fn new(
        transform: Affine3A,
        tint: [u8; 4],
        material: Material,
        array: TextureArray,
        mesh: Mesh,
    ) -> Self {
        Self {
            model: rows(transform),
            uv: [0; 4],
            layer: 0,
            material: material as u8,
            flags: (array as u8) | ((mesh as u8) << 2),
            tint,
            params: [0.0; 4],
        }
    }
}

/// `transform` as three rows of four numbers.
fn rows(transform: Affine3A) -> [f32; 12] {
    let m = transform.matrix3;
    let t = transform.translation;
    [
        m.x_axis.x, m.y_axis.x, m.z_axis.x, t.x, //
        m.x_axis.y, m.y_axis.y, m.z_axis.y, t.y, //
        m.x_axis.z, m.y_axis.z, m.z_axis.z, t.z,
    ]
}
