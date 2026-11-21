//! The draw record: one fixed-size record per shape, which the renderer reads in order
//! (docs/beryl-3d.md, "The frame"). What `uv`, `layer` and `params` mean depends on the
//! material, so records are made only by the constructors here, one per material, and each
//! fills only the fields the doc's table gives that material. No code outside this file writes
//! `uv` or `params`.

use glam::{Affine3A, Vec2};

use kit_base::atlas::FaceSize;
use kit_base::paint::Geometry;
use kit_base::paint::Shadow;

/// One shape to draw. 80 bytes, laid out as the renderer's instance buffer expects.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
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
    BoxShadow = 9,
    Gradient = 10,
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
    /// The part of that square the `sprites::OUTLINE` shape covers: the ring between the
    /// rectangles `sprites::OUTLINE_OUTER` and `sprites::OUTLINE_HOLE`. An outline drawn as
    /// a whole square would shade the empty card-sized middle and the empty sides of its cell
    /// too, three quarters of its pixels.
    Outline = 3,
}

/// A rectangle of one atlas page: `rect` is `[x, y, width, height]` in texels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sprite {
    pub page: u16,
    pub rect: [u16; 4],
}

/// The shapes on the atlas's first page, which light and effects draw with: a 1024 by 1024 page
/// cut into 256 by 256 cells, row by row from the top left.
pub mod sprites {
    use glam::Vec2;

    use super::Sprite;

    /// The atlas page that holds them.
    pub const PAGE: u16 = 0;
    /// The side of one cell, in texels.
    pub const CELL: u16 = 256;

    const fn cell(x: u16, y: u16) -> Sprite {
        Sprite {
            page: PAGE,
            rect: [x, y, CELL, CELL],
        }
    }

    pub const DISC: Sprite = cell(0, 0);
    pub const BURST: Sprite = cell(256, 0);
    pub const RING: Sprite = cell(512, 0);
    pub const CONE: Sprite = cell(768, 0);
    pub const COMET: Sprite = cell(0, 256);
    /// The cell that holds the ribbons' shapes, side by side in columns, each the same all
    /// along: `RIBBON`, `RIBBON_CORE` and the columns of `ribbon_lit`.
    pub const RIBBON_CELL: Sprite = cell(256, 256);
    /// A ribbon running left to right: a soft band, the same all along. Columns 8 to 56 of
    /// `RIBBON_CELL`.
    pub const RIBBON: Sprite = ribbon_columns(8, 48);
    /// The hot core of an arrow's ribbon, which the web's ribbon shader lights in the `core`
    /// color of its tone: narrower than the ribbon's body, a quad as high as
    /// `light.glow.core_reach` times the body's width. Columns 80 to 112 of `RIBBON_CELL`.
    pub const RIBBON_CORE: Sprite = ribbon_columns(80, 32);
    /// The first column of `RIBBON_CELL` that `ribbon_lit` draws from.
    pub const RIBBON_LIT_FIRST: u16 = 128;
    /// How many columns `ribbon_lit` draws from, each the body and halo of a ribbon at one
    /// share of its body in its quad's height.
    pub const RIBBON_LIT_COLUMNS: u16 = 120;
    /// The largest share of its quad's height that a ribbon's body takes, which the last of
    /// `ribbon_lit`'s columns draws: a quad is `light.glow.reach_share` times the body's width
    /// and more, so the share is less than its inverse.
    pub const RIBBON_LIT_MOST: f32 = 0.4;

    const fn ribbon_columns(first: u16, count: u16) -> Sprite {
        Sprite {
            page: PAGE,
            rect: [
                RIBBON_CELL.rect[0] + first,
                RIBBON_CELL.rect[1],
                count,
                CELL,
            ],
        }
    }

    /// The column boundary of `ribbon_lit`'s columns where a ribbon's body is `share` of its
    /// quad's height (from 0 to `RIBBON_LIT_MOST`): a sampler filtering there blends the two
    /// columns beside it, whose shares lie half a column below and above. It is never the
    /// first or last boundary, which the columns beyond them would blend into.
    #[must_use]
    pub fn ribbon_lit_column(share: f32) -> u16 {
        let last = f32::from(RIBBON_LIT_COLUMNS - 1);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a column from 1 to RIBBON_LIT_COLUMNS - 1"
        )]
        let column = (share / RIBBON_LIT_MOST * f32::from(RIBBON_LIT_COLUMNS))
            .round()
            .clamp(1.0, last) as u16;
        column
    }

    /// The columns of `RIBBON_CELL` that draw a ribbon's body and halo as the web's ribbon
    /// shader does, from the column boundary `from` to `to` (`ribbon_lit_column`, `from` no
    /// greater than `to`): one record of the ribbon's tone, on a quad as high as the halo
    /// reaches, lights both, its body's share of the quad changing along its length from one
    /// to the other.
    #[must_use]
    pub const fn ribbon_lit(from: u16, to: u16) -> Sprite {
        ribbon_columns(RIBBON_LIT_FIRST + from, to - from)
    }

    /// The share of its quad's height that the body of column `column` of `ribbon_lit` takes.
    #[must_use]
    pub fn ribbon_lit_share(column: u16) -> f32 {
        (f32::from(column) + 0.5) / f32::from(RIBBON_LIT_COLUMNS) * RIBBON_LIT_MOST
    }
    pub const OUTLINE: Sprite = cell(512, 256);
    pub const HEAD: Sprite = cell(768, 256);
    pub const WEDGE: Sprite = cell(0, 512);
    pub const CRESCENT: Sprite = cell(256, 512);
    pub const TAP: Sprite = cell(512, 512);
    pub const MOON: Sprite = cell(768, 512);
    pub const PLAQUE: Sprite = cell(0, 768);
    pub const COIN: Sprite = cell(256, 768);
    pub const HALO: Sprite = cell(512, 768);
    /// The white-hot core of `WEDGE`: its rim, hottest at the point, which the web's
    /// `fighterLight` lights in the `core` color of the wedge's tone.
    pub const WEDGE_HOT: Sprite = cell(768, 768);

    /// How much room `WEDGE` and `WEDGE_HOT` leave round the chevron for its halo, on each side,
    /// as a share of the edge it lies against (across) and in card widths (along). The web
    /// draws the chevron's box 26 pixels larger on each side (`glow.ts`, `room`), about 0.27 of
    /// the width of a card on the desktop table.
    pub const WEDGE_ROOM: f32 = 0.27;

    /// The size to scale a quad drawn with `WEDGE` or `WEDGE_HOT` to, so that the chevron is
    /// `across` wide at its base and `length` from its base to its point, with room round it
    /// for its halo; all three in the units of `card_width`, the width of the card it points
    /// from. The quad's middle lies `length / 2` past the base, toward the point.
    #[must_use]
    pub fn wedge_size(across: f32, length: f32, card_width: f32) -> Vec2 {
        Vec2::new(
            across * (1.0 + 2.0 * WEDGE_ROOM),
            length + 2.0 * WEDGE_ROOM * card_width,
        )
    }

    /// Half the width and height of the rectangle outside which `OUTLINE`'s cell is empty, in
    /// the units of a unit quad drawn with the whole cell (-0.5 to 0.5 across). The outline is
    /// card-shaped, narrower than its square cell.
    pub const OUTLINE_OUTER: [f32; 2] = [0.37, 0.5];
    /// Half the width and height of the rectangle inside which `OUTLINE`'s cell is empty, in
    /// the same units: the middle of the outline, where the card it rings shows through.
    /// `beryl_assets`' tests hold both rectangles to the shape it draws, with a margin for
    /// filtering.
    pub const OUTLINE_HOLE: [f32; 2] = [0.29, 0.42];
    /// Half the width and height of the rounded rectangle that `OUTLINE`'s line runs along, in
    /// the same units. It has the proportions of the card outline drawn in the cell.
    pub const OUTLINE_LINE: [f32; 2] = [LINE_HEIGHT * DRAWN_CARD_SHAPE, LINE_HEIGHT];
    /// The width per height of the card that `beryl_assets` draws the outline and the halo
    /// around in their atlas cells: a printed card's 63 by 88 millimetres. The atlas is artwork,
    /// drawn once when the renderer starts, so this is the artwork's shape, not the card size the
    /// data sets; the quads stretch the artwork over the card they ring.
    const DRAWN_CARD_SHAPE: f32 = 63.0 / 88.0;
    /// `OUTLINE_LINE`'s height: 0.94 of the cell's drawn area, which leaves `beryl_assets`'
    /// border of 2 texels on each side of the 256 texel cell.
    const LINE_HEIGHT: f32 = 0.94 * 126.0 / 256.0;

    /// Half the width and height of the card that `HALO`'s light lies round, in the units of a
    /// unit quad drawn with the whole cell (-0.5 to 0.5 across): the halo is empty inside it,
    /// and its light reaches out from its edge to near the edge of the cell.
    pub const HALO_CARD: [f32; 2] = [HALO_HEIGHT * DRAWN_CARD_SHAPE, HALO_HEIGHT];
    /// `HALO_CARD`'s height.
    const HALO_HEIGHT: f32 = 0.32;

    /// The size to scale a quad drawn with `HALO` to, so that the card it lies round is `half`
    /// from its middle across and up.
    #[must_use]
    pub fn halo_size(half: Vec2) -> Vec2 {
        half / Vec2::from(HALO_CARD)
    }

    /// The size to scale a quad drawn with `OUTLINE` to, so that its line runs `half` from its
    /// middle across and up: around a card when `half` is the card's half size. The cell already
    /// holds a card's proportions, so a quad scaled to the card's own size drew the line at
    /// about seven tenths of the card's width.
    #[must_use]
    pub fn outline_size(half: Vec2) -> Vec2 {
        half / Vec2::from(OUTLINE_LINE)
    }
}

const MESH_SHIFT: u8 = 2;
const MESH_BITS: u8 = 0b1100;
const CLIP_SHIFT: u8 = 4;

impl Draw {
    /// A solid shape of the table material, drawn in its tint. It reads no `uv` or `layer`, and
    /// `params` is zero.
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

    /// The table's cloth, painted with the web interface's `--tabletop`: a radial gradient
    /// spanning the window, from warm brown in the middle to near black in the corners, darkened
    /// by its grain. The gradient lies on the screen, not on the cloth, as the web's lies on its
    /// page, so the cloth's place and size do not move it. The gradient is multiplied by `tint`,
    /// so white draws it as the web does. `params.x` is 1.
    #[must_use]
    pub fn tabletop(transform: Affine3A, tint: [u8; 4]) -> Self {
        let mut draw = Self::table(transform, tint);
        draw.params = [1.0, 0.0, 0.0, 0.0];
        draw
    }

    /// The same tabletop record, darkened along the table's far edge as the web's `.far-shade`
    /// darkens it: a radial gradient of near black, from 80% opaque at `centre` to nothing at
    /// the ellipse `radius` across, both in physical pixels of the target (`table::far_shade`).
    /// The shader holds the gradient's stops and the ratio of its radii. `params` becomes
    /// `[1, centre.x, centre.y, radius]`; a radius of 0 draws no shade.
    #[must_use]
    pub fn with_far_shade(mut self, centre: Vec2, radius: f32) -> Self {
        debug_assert!(
            self.material == Material::Table as u8 && self.params[0] > 0.5,
            "only the tabletop carries the far shade"
        );
        self.params = [1.0, centre.x, centre.y, radius];
        self
    }

    /// A card showing `face` on its front and `back` on its back, both layers of the array for
    /// `size`. The back's layer goes in `uv[0]`, because a face fills its layer and needs no
    /// rectangle. `dissolve` is how far the card has broken up and `whiten` how far its face is
    /// replaced by white light, each from 0 to 1. `desaturate` (`params.z`, 0 here) is set by
    /// `Draw::desaturated`.
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

    /// The same card record with its face's colors moved `amount` of the way, from 0 to 1, to
    /// their luminance (`params.z`), as CSS's `grayscale()` and `saturate()` filters move them.
    #[must_use]
    pub fn desaturated(mut self, amount: f32) -> Self {
        debug_assert_eq!(
            self.material,
            Material::Card as u8,
            "only a card desaturates"
        );
        self.params[2] = amount;
        self
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

    /// A glow `progress` of the way through, from 0 to 1. The outline is drawn with the mesh
    /// that leaves out the parts of its cell it leaves empty, which shades the same pixels.
    #[must_use]
    pub fn glow(transform: Affine3A, tint: [u8; 4], sprite: Sprite, progress: f32) -> Self {
        let mut draw = Self::sprite(transform, tint, Material::Glow, sprite);
        draw.params = [progress, 0.0, 0.0, 0.0];
        if sprite == sprites::OUTLINE {
            draw.flags = (draw.flags & !MESH_BITS) | ((Mesh::Outline as u8) << MESH_SHIFT);
        }
        draw
    }

    /// The same glow on a quad that narrows along its length (its local x), from `start` of its
    /// height at its start to `end` at its end, each from 0 to 1, with its sprite narrowing
    /// with it (`params.y` and `params.z`, which the vertex shader's `taper` reads). The pieces
    /// of a ribbon drawn this way meet with no step in width.
    #[must_use]
    pub fn tapered(mut self, start: f32, end: f32) -> Self {
        debug_assert_eq!(self.material, Material::Glow as u8, "only a glow tapers");
        self.params[1] = start.clamp(1e-3, 1.0);
        self.params[2] = end.clamp(1e-3, 1.0);
        self
    }

    /// The same glow with its two ends cut aslant: each end moves along the quad by `start` or
    /// `end` lengths across it for each length across (from -1 to 1, in 2048 steps), so that
    /// the pieces of a bending ribbon meet on the line that halves the turn between them
    /// (`params.w`, which the vertex shader's `on_record` reads).
    #[must_use]
    pub fn mitred(mut self, start: f32, end: f32) -> Self {
        debug_assert_eq!(self.material, Material::Glow as u8, "only a glow is mitred");
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a step from 0 to 2047"
        )]
        let step = |slant: f32| ((slant.clamp(-1.0, 1.0) + 1.0) * 1023.5).round() as u32;
        #[expect(
            clippy::cast_precision_loss,
            reason = "below 2^23, which a float holds exactly"
        )]
        let code = (step(start) * 2048 + step(end)) as f32;
        self.params[3] = code + 1.0;
        self
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

    /// A panel as `panel` makes one, whose border's sides differ in width or color: it draws the
    /// fill in `tint` and, in the color `border`, only the sides of `widths` (top, right, bottom
    /// and left, in logical points) whose bits `sides` sets (`SIDE_TOP` and the rest). A border
    /// of several colors is one record for each color, the first carrying the fill and the others
    /// a clear tint. The widths go in the high bytes of `uv` in quarter points, up to 63.75, with
    /// the color in the low bytes, and `params.w`, a uniform border's width, holds the side bits
    /// negated, which a width never is.
    #[must_use]
    pub fn panel_sides(
        transform: Affine3A,
        tint: [u8; 4],
        border: [u8; 4],
        size: Vec2,
        radius: f32,
        (widths, sides): ([f32; 4], u8),
    ) -> Self {
        let mut draw = Self::panel(transform, tint, border, size, radius, 0.0);
        for (slot, width) in draw.uv.iter_mut().zip(widths) {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped to a byte"
            )]
            let quarters = (width * 4.0).round().clamp(0.0, 255.0) as u16;
            *slot |= quarters << 8;
        }
        draw.params[3] = -f32::from(sides & 0b1111);
        draw
    }

    /// One CSS box shadow of a flat-layer element whose border box, or padding box for an inset
    /// shadow, is `size` logical points large with corners of `radius`. `shadow` gives its
    /// offset, spread and blur, which `Shadow::sigma` turns into a standard deviation, and
    /// `shape_radius` is the corner radius of the shadow's shape, the box grown by the spread.
    /// The offsets, spread and shape radius go in `uv` as `fixed` lengths, and `layer` is 1 for
    /// an inset shadow. `transform` takes the unit quad to the box moved by the offset and grown
    /// by `box_shadow_reach`.
    #[must_use]
    pub fn box_shadow(
        transform: Affine3A,
        tint: [u8; 4],
        size: Vec2,
        radius: f32,
        shadow: &Shadow,
        shape_radius: f32,
    ) -> Self {
        let mut draw = Self::new(
            transform,
            tint,
            Material::BoxShadow,
            TextureArray::Atlas,
            Mesh::Quad,
        );
        draw.uv = [
            fixed(shadow.x),
            fixed(shadow.y),
            fixed(shadow.spread),
            fixed(shape_radius),
        ];
        draw.layer = u16::from(shadow.inset);
        draw.params = [size.x, size.y, radius, shadow.sigma()];
        draw
    }

    /// A rounded rectangle of the flat layer, `size` logical points large with corners of
    /// `radius`, filled with entry `entry` of the frame's gradients (`Frame::paints`) along
    /// `geometry`, measured from the rectangle's top left corner, and its edges blurred with a
    /// Gaussian of standard deviation `sigma` points (0 for sharp ones). The tint's opacity
    /// scales the gradient's. A radial gradient's centre and radii go in `uv` as `fixed`
    /// lengths, and a linear one's angle in `uv[0]` in 65,536ths of a turn; `layer` is the entry,
    /// plus `RADIAL` for a radial gradient. `transform` takes the unit quad to the rectangle grown
    /// by `gradient_reach(sigma)` on every side.
    #[must_use]
    pub fn gradient(
        transform: Affine3A,
        tint: [u8; 4],
        size: Vec2,
        radius: f32,
        sigma: f32,
        geometry: &Geometry,
        entry: u16,
    ) -> Self {
        let mut draw = Self::new(
            transform,
            tint,
            Material::Gradient,
            TextureArray::Atlas,
            Mesh::Quad,
        );
        match *geometry {
            Geometry::Linear { angle } => {
                let turns = angle.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a fraction of a turn, from 0 to 1, times 65,536 wraps to u16"
                )]
                let angle = ((turns * 65_536.0).round() as u32 % 65_536) as u16;
                draw.uv = [angle, 0, 0, 0];
                draw.layer = entry;
            }
            Geometry::Radial { centre, radii } => {
                draw.uv = [
                    fixed(centre.x),
                    fixed(centre.y),
                    fixed(radii.x),
                    fixed(radii.y),
                ];
                draw.layer = entry | RADIAL;
            }
        }
        draw.params = [size.x, size.y, radius, sigma];
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

    /// The strip that softens the edge of this card, drawn with the blended records: the same
    /// card drawn with the edge strip mesh, which fades from the card's outline to one pixel
    /// outside it.
    #[must_use]
    pub fn edge(mut self) -> Self {
        debug_assert_eq!(
            self.material,
            Material::Card as u8,
            "only a card has an edge strip"
        );
        self.flags = (self.flags & !MESH_BITS) | ((Mesh::CardEdge as u8) << MESH_SHIFT);
        self
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
            flags: (array as u8) | ((mesh as u8) << MESH_SHIFT),
            tint,
            params: [0.0; 4],
        }
    }
}

/// How far past the box a box shadow's quad reaches on each side, in logical points: the spread
/// and three standard deviations of its blur, past which the Gaussian leaves under a level of
/// 255. An inset shadow's quad is the padding box itself. The shader works out the same quad
/// from the record's `params` and `uv`, so the two must agree.
#[must_use]
pub fn box_shadow_reach(shadow: &Shadow) -> f32 {
    if shadow.inset {
        0.0
    } else {
        shadow.spread + 3.0 * shadow.sigma()
    }
}

/// The bits of a `Draw::panel_sides` record's side mask that name the sides it draws.
pub const SIDE_TOP: u8 = 1;
pub const SIDE_RIGHT: u8 = 2;
pub const SIDE_BOTTOM: u8 = 4;
pub const SIDE_LEFT: u8 = 8;

/// What a `Gradient` record's `layer` is raised by when its gradient is radial.
pub const RADIAL: u16 = 1 << 15;

/// How far past its rectangle a `Gradient` record's quad reaches on each side, in logical
/// points: three standard deviations of its blur, past which the Gaussian leaves under a level of
/// 255. The shader works out the same quad from the record's `params`, so the two must agree.
#[must_use]
pub fn gradient_reach(sigma: f32) -> f32 {
    3.0 * sigma.max(0.0)
}

/// A length in logical points as a `u16` of sixteenths of a point plus 32,768, which covers
/// 2,048 points either way.
#[must_use]
pub fn fixed(points: f32) -> u16 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to the range of u16 first"
    )]
    let value = (points * 16.0 + 32_768.0).round().clamp(0.0, 65_535.0) as u16;
    value
}

/// `transform` as three rows of four numbers, as `Draw::model` holds it.
#[must_use]
pub fn rows(transform: Affine3A) -> [f32; 12] {
    let m = transform.matrix3;
    let t = transform.translation;
    [
        m.x_axis.x, m.y_axis.x, m.z_axis.x, t.x, //
        m.x_axis.y, m.y_axis.y, m.z_axis.y, t.y, //
        m.x_axis.z, m.y_axis.z, m.z_axis.z, t.z,
    ]
}
