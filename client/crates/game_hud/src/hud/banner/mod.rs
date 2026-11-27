//! The step banner, as the web interface's `StepBanner` shows it (`ui/src/game/StepBanner.tsx`,
//! `combatBanner` in `ui/src/game/stepLabels.ts`): during combat, "Combat" over what is being
//! declared, what has been, or the damage, such as "Combat" over "Blockers declared", in the
//! band between the opponent's life orb and their permanents.
//!
//! - The banner's duration is set by `banner_ms` in `hud.json`. It starts on the first frame
//!   whose view at rest calls for it. It can show again after the view stops calling for it,
//!   as in the web's `useBannerUp`.
//! - The banner is a float (`floats::STEP_BANNER`), laid out ahead (`Prepare`), so that
//!   the frame that shows or hides it lays nothing out and allocates nothing.
//! - The gold ring under the opponent's orb while the AI decides (`floats::THINKING`) lies where
//!   the banner does, so it hides while the banner is up, during the opening, and at a manual
//!   table. It is also laid out ahead (`Prepare`) and pulses via `FloatLook` without allocating.

use beryl_protocol::protocol::{Combat, Kind, PlayerView, Side, Status, Step};
use glam::Vec2;

use game_table::table::HudAnchor;
use kit_base::{data::hud::Hud, tokens::size};
use kit_scene::{Context, Feature, StateUpdate};
use kit_ui::element::{Element, FloatId, floats};
use std::convert::Infallible;
use std::f32::consts::TAU;

mod view;
use kit_ui::layout::{FloatLook, Layer};

/// The banner's title, the same for every detail.
const TITLE: &str = "Combat";

/// Every detail a banner shows, in the order `combat_banner` tells them.
const DETAILS: [&str; 5] = [
    "Declare attackers",
    "Declare blockers",
    "Attackers declared",
    "Blockers declared",
    "Combat damage",
];

/// The key the thinking ring is laid out under.
const RING: u64 = 0;

/// What the banner and the thinking ring show now.
#[derive(Debug, Default)]
pub struct Banner {
    /// The detail the view at rest calls for, by index in `DETAILS`, and since when.
    wanted: Option<(usize, u64)>,
    /// The detail the banner float shows, if any.
    shown: Option<usize>,
    /// Whether the thinking ring float is currently shown.
    thinking: bool,
    /// When the thinking ring began showing, on the frame clock.
    thinking_since: Option<u64>,
}

impl Feature for Banner {
    const SOURCE: &'static str = file!();

    type State = Self;
    type Message = Infallible;
    type Inputs<'a> = usize;
    type ViewKey = usize;
    type Command = Infallible;
    type Request = Infallible;

    fn update(
        _: &mut Self,
        message: &Infallible,
        _: &mut Context<'_, Infallible, Infallible>,
    ) -> bool {
        match *message {}
    }

    fn view_key(_: &Self, detail: &usize) -> usize {
        *detail
    }

    fn view(_: &Self, detail: usize) -> Element {
        view::banner(detail)
    }
}

impl super::FloatFeature for Banner {
    fn floats() -> impl Iterator<Item = FloatId> {
        [floats::STEP_BANNER, floats::THINKING].into_iter()
    }
}

/// Lays out the banner and thinking ring before their view becomes visible.
pub struct Prepare<'a> {
    pub layer: &'a mut Layer,
    pub view: &'a PlayerView,
}

impl StateUpdate<Banner> for Prepare<'_> {
    type Output = ();

    fn apply(self, state: &mut Banner) -> (bool, ()) {
        if let Some(detail) = combat_banner(self.view) {
            self.layer.prepare(floats::STEP_BANNER, detail as u64, || {
                Banner::view(state, detail)
            });
        }
        if matches!(self.view.status, Status::Thinking) {
            self.layer
                .prepare(floats::THINKING, RING, view::thinking_ring);
        }
        (false, ())
    }
}

/// Samples the banner and thinking ring for the view shown at rest.
pub struct Draw<'a> {
    pub layer: &'a mut Layer,
    pub view: Option<&'a PlayerView>,
    pub latest: Option<&'a PlayerView>,
    pub now: u64,
    pub visible: bool,
    pub allow_thinking: bool,
    pub orb_scale: f32,
    pub tuning: Hud,
}

impl StateUpdate<Banner> for Draw<'_> {
    type Output = bool;

    fn apply(self, state: &mut Banner) -> (bool, bool) {
        state.write(
            self.layer,
            self.view,
            self.latest,
            self.now,
            self.visible,
            self.allow_thinking,
            self.orb_scale,
            self.tuning,
        );
        (false, state.shown.is_some())
    }
}

/// The step banner `view` calls for during combat, as the web's `combatBanner`: its detail, by
/// index in `DETAILS`.
#[must_use]
pub fn combat_banner(view: &PlayerView) -> Option<usize> {
    let kind = match &view.status {
        Status::Deciding { decision } => Some(decision.kind),
        _ => None,
    };
    let fighting =
        (view.objects.iter()).any(|object| matches!(object.combat, Some(Combat::Attacking { .. })));
    match (kind, view.turn.step) {
        (Some(Kind::Attacker | Kind::AttackTarget), _) => Some(0),
        (Some(Kind::Blocker | Kind::BlockTarget), _) => Some(1),
        _ if !fighting => None,
        (_, Step::DeclareAttackers) => Some(2),
        (_, Step::DeclareBlockers) => Some(3),
        (_, Step::FirstStrikeDamage | Step::CombatDamage) => Some(4),
        _ => None,
    }
}

/// The words of the banner `combat_banner` calls `detail`: its title and its detail.
#[must_use]
pub fn words(detail: usize) -> (&'static str, &'static str) {
    (TITLE, DETAILS[detail.min(DETAILS.len() - 1)])
}

impl Banner {
    /// Whether the thinking ring is currently shown and pulsing.
    #[must_use]
    pub fn thinking(&self) -> bool {
        self.thinking
    }

    /// Whether the banner is up at `now`.
    #[must_use]
    fn up(&self, now: u64, duration: u64) -> bool {
        self.wanted
            .is_some_and(|(_, since)| now.saturating_sub(since) < duration)
    }

    /// Shows the banner and thinking ring for the view at rest at the given time while the table
    /// is visible. Lays nothing out and allocates nothing.
    #[expect(
        clippy::too_many_arguments,
        reason = "sampling inputs for banner and thinking floats"
    )]
    fn write(
        &mut self,
        layer: &mut Layer,
        view: Option<&PlayerView>,
        latest: Option<&PlayerView>,
        now: u64,
        visible: bool,
        allow_thinking: bool,
        orb_scale: f32,
        tuning: Hud,
    ) {
        let wants = view.and_then(combat_banner);
        match (wants, self.wanted) {
            (Some(detail), Some((held, _))) if detail == held => {}
            (Some(detail), _) => self.wanted = Some((detail, now)),
            (None, _) => self.wanted = None,
        }
        let up = self.up(now, tuning.banner_ns());
        let show = self
            .wanted
            .filter(|_| visible && up)
            .map(|(detail, _)| detail);
        if show != self.shown {
            layer.show_prepared(floats::STEP_BANNER, show.map(|detail| detail as u64));
            self.shown = show;
        }
        if show.is_some() {
            layer.anchor_float(
                floats::STEP_BANNER,
                HudAnchor::CombatBanner.at(
                    Vec2::ZERO,
                    Vec2::new(0.5, 0.0),
                    Vec2::Y * (size::ORB_HEIGHT / 2.0 + size::ORB_BEZEL + tuning.banner_gap),
                ),
                FloatLook::default(),
            );
        }
        let thinking = visible
            && allow_thinking
            && !up
            && view.is_some_and(|view| matches!(view.status, Status::Thinking))
            && latest.is_some_and(|view| matches!(view.status, Status::Thinking));
        if thinking != self.thinking {
            layer.show_prepared(floats::THINKING, thinking.then_some(RING));
            self.thinking = thinking;
            self.thinking_since = thinking.then_some(now);
        }
        if thinking {
            let style = tuning.thinking;
            let elapsed = now.saturating_sub(self.thinking_since.unwrap_or(now)) as f32;
            let wave = 0.5 + 0.5 * (elapsed / style.pulse_ns() * TAU).cos();
            let opacity = style.min_opacity + (1.0 - style.min_opacity) * wave;
            let scale = orb_scale * (style.min_scale + (1.0 - style.min_scale) * wave);
            let offset_y =
                (size::ORB_HEIGHT / 2.0 + size::ORB_BEZEL + style.gap + size::THINKING / 2.0)
                    * orb_scale;
            layer.anchor_float(
                floats::THINKING,
                HudAnchor::Life(Side::Opponent).at(
                    Vec2::ZERO,
                    Vec2::new(0.5, 0.5),
                    Vec2::Y * offset_y,
                ),
                FloatLook {
                    opacity,
                    scale,
                    ..FloatLook::default()
                },
            );
        }
    }
}
