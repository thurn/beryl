//! Optional adjustments to a profile's table geometry for a screen projection.

use beryl_protocol::protocol::RowName;
use glam::Vec2;
use kit_base::data::{Card, profiles::ProfileData};
use kit_base::pose::Pose;
use kit_space::arrange::row::ScaleLimit;

use super::{Camera, Layout, RowArea, Window};

/// The profile's region fit. The default preserves the profile's geometry and card poses.
#[derive(Clone, Copy, Debug, Default)]
pub struct RegionFit(Option<&'static RegionPolicy>);

/// Computes the regions for the current profile and window, and adjusts each visible row card
/// to that projection. A policy can leave a profile's regions unchanged.
#[derive(Debug)]
pub struct RegionPolicy {
    pub regions: fn(&ProfileData, &Layout, Window) -> Option<FittedRegions>,
    pub screen: fn(Window) -> ScreenProjection,
    pub row_pose: fn(&FittedRow, f32, &Camera, Pose, bool) -> Pose,
}

impl RegionFit {
    #[must_use]
    pub const fn new(policy: &'static RegionPolicy) -> Self {
        Self(Some(policy))
    }

    pub(crate) fn apply(self, profile: &ProfileData, window: Window, layout: &mut Layout) {
        layout.region_fit = self;
        if let Some(policy) = self.0
            && let Some(fit) = (policy.regions)(profile, layout, window)
        {
            layout.fitted = Some(fit.rows);
            layout.row_zoom = Some(fit.zoom);
            layout.fitted_piles = Some(fit.piles);
            layout.pile_scale = fit.piles.scale;
            layout.shared_pile_scale = fit.piles.shared_scale;
            layout.tabletop_shade = fit.tabletop_shade;
        }
    }

    pub(crate) fn screen(self, window: Window) -> ScreenProjection {
        self.0
            .map_or(ScreenProjection::IDENTITY, |policy| (policy.screen)(window))
    }

    pub(crate) fn row_pose(
        self,
        row: &FittedRow,
        zoom: f32,
        camera: &Camera,
        pose: Pose,
        tapped: bool,
    ) -> Pose {
        self.0.map_or(pose, |policy| {
            (policy.row_pose)(row, zoom, camera, pose, tapped)
        })
    }
}

impl PartialEq for RegionFit {
    fn eq(&self, other: &Self) -> bool {
        match (self.0, other.0) {
            // Policies live in statics. Compare the policy's identity without comparing
            // function addresses, which can differ between compilation units.
            (Some(left), Some(right)) => std::ptr::eq(left, right),
            (None, None) => true,
            _ => false,
        }
    }
}

/// Where a reference screen sits in the native window and how large it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenProjection {
    /// The reference screen's top left in window points.
    pub origin: Vec2,
    /// Window points per reference point.
    pub scale: f32,
    /// The fitted reference dimensions before scaling, when a reference stage is available.
    pub stage_size: Option<kit_base::geometry::Size>,
}

impl ScreenProjection {
    const IDENTITY: Self = Self {
        origin: Vec2::ZERO,
        scale: 1.0,
        stage_size: None,
    };
}

/// Screen-projected overrides for a profile's rows, piles and cloth shade.
pub struct FittedRegions {
    pub rows: FittedRows,
    pub zoom: RowZoom,
    pub piles: FittedPiles,
    pub tabletop_shade: Option<TabletopShade>,
}

/// An elliptical shade painted into the cloth, in physical pixels from the target's top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabletopShade {
    pub centre: Vec2,
    pub radii: Vec2,
}

/// Each side's creatures, lands and other permanents rows, the viewer's first.
pub type FittedRows = [[FittedRow; 3]; 2];

/// One row projected onto the table, with sizes and gaps in units of an unscaled card.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FittedRow {
    /// The row's centre line and horizontal limits, in table millimetres.
    pub area: RowArea,
    /// Card scale when the row has room for every card at full size.
    pub scale: f32,
    pub gap: f32,
    /// The gap before the first tapped card, at full size.
    pub tapped_gap: f32,
    /// How far the row's plane leans up from the table, in radians.
    pub tilt: f32,
    /// Signed table distance an attacker steps out of the row, untapped then tapped.
    pub advance: [f32; 2],
    /// Projected width of an untapped card in logical points, at full size.
    pub width: f32,
    pub projection: RowProjection,
}

/// The reference image's coordinates and vanishing point, used to fit row card outlines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowProjection {
    /// The offset added to a reference point before scaling it to window points.
    pub offset: Vec2,
    /// Window points per reference point.
    pub scale: f32,
    /// The vanishing point in reference coordinates.
    pub vanishing_point: Vec2,
}

impl FittedRow {
    /// The height of the row's plane at `y`, hinged on the near edge of an untapped card.
    #[must_use]
    pub fn height(&self, y: f32, card: Card) -> f32 {
        let half = card.height / 2.0 * self.scale;
        half * self.tilt.sin() + (y - self.area.y) * self.tilt.tan()
    }
}

/// How far a crowded row can zoom the battlefield out and shrink its own cards.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowZoom {
    /// The least zoom the rows share.
    pub minimum: f32,
    /// The creatures' rows' own limit, or `None` when they take the shared zoom.
    pub creatures: Option<ScaleLimit>,
    pub back: ScaleLimit,
}

impl RowZoom {
    /// Each row shrinks on its own, as far as it needs to fit its room, down to `least`. No
    /// row shrinks another.
    #[must_use]
    pub const fn independent(least: f32) -> Self {
        let limit = ScaleLimit {
            minimum: least,
            shared_minimum: 1.0,
        };
        Self {
            minimum: 1.0,
            creatures: Some(limit),
            back: limit,
        }
    }

    /// The least zoom of `row`.
    #[must_use]
    pub fn least(&self, row: RowName) -> f32 {
        match row {
            RowName::Creatures => self.creatures.map_or(self.minimum, |limit| limit.minimum),
            _ => self.back.minimum,
        }
    }
}

/// Piles placed by the projection of their top cards. Each array starts with the viewer's side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FittedPiles {
    /// The top card's centre in logical points, ordered library, graveyard and exile.
    pub tops: [[Vec2; 3]; 2],
    pub scale: [f32; 2],
    pub tilt: [f32; 2],
    /// The skew of each pile's cards, ordered as `tops`.
    pub skew: [[f32; 3]; 2],
    /// The shared piles' top centres in logical points, ordered library, graveyard and exile.
    pub shared_tops: [Vec2; 3],
    pub shared_scale: f32,
    pub shared_tilt: f32,
    pub shared_skew: [f32; 3],
    /// Scales the offsets of exposed card edges into the current window.
    pub rim_scale: Vec2,
}
