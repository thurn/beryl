//! The step banner, as the web interface's `StepBanner` shows it (`ui/src/game/StepBanner.tsx`,
//! `combatBanner` in `ui/src/game/stepLabels.ts`): during combat, "Combat" over what is being
//! declared, what has been, or the damage, such as "Combat" over "Blockers declared", in the
//! band between the opponent's life orb and their permanents.
//!
//! - It shows for `hud.banner` milliseconds (the web's `BANNER_MS`) on the show's clock from the
//!   first frame whose view at rest calls for it, then hides until that view no longer calls for
//!   it and a later one calls for a banner again, as the web's `useBannerUp`.
//! - The ring under the opponent's orb while they decide (the web's `Thinking`) lies where the
//!   banner does, so it hides while the banner is up. Both are floats (`floats::STEP_BANNER`,
//!   `floats::THINKING`), laid out ahead (`Banner::prepare`), so that the frame that shows or
//!   hides one lays nothing out and allocates nothing.

use beryl_protocol::protocol::{Combat, Kind, PlayerView, Side, Status, Step};
use glam::Vec2;

use super::players::orb_centre;
use crate::data::space::hud::{BANNER, BANNER_GAP, THINKING_GAP};
use crate::element::{Align, Element, Justify, column, floats, text};
use crate::frame::Role;
use crate::layout::{FloatLook, Layer};
use crate::table::Window;
use crate::tokens::{color, size, type_style};

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
    thinking: bool,
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

/// How long the banner shows, in nanoseconds.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a banner shows for a few seconds"
)]
const SHOWS: u64 = (BANNER * 1_000_000.0) as u64;

impl Banner {
    /// Lays out in `layer` the banner `view` will call for once the table shows it, and the
    /// thinking ring if the opponent decides in it, which `write` then shows without laying
    /// anything out. Lays out nothing that is laid out already, so a game lays out each banner
    /// once at most.
    pub fn prepare(layer: &mut Layer, view: &PlayerView) {
        if let Some(detail) = combat_banner(view) {
            layer.prepare(floats::STEP_BANNER, detail as u64, || banner(detail));
        }
        if matches!(view.status, Status::Thinking) {
            layer.prepare(floats::THINKING, RING, thinking_ring);
        }
    }

    /// Whether the banner is up at `now`.
    #[must_use]
    pub fn up(&self, now: u64) -> bool {
        self.wanted
            .is_some_and(|(_, since)| now.saturating_sub(since) < SHOWS)
    }

    /// Shows in `layer` the banner and the thinking ring that `view`, the view at rest, calls for
    /// at `now` in `window`, or neither unless `visible`. Lays nothing out and allocates nothing.
    pub fn write(
        &mut self,
        layer: &mut Layer,
        view: Option<&PlayerView>,
        now: u64,
        window: Window,
        visible: bool,
    ) {
        let wants = view.and_then(combat_banner);
        match (wants, self.wanted) {
            (Some(detail), Some((held, _))) if detail == held => {}
            (Some(detail), _) => self.wanted = Some((detail, now)),
            (None, _) => self.wanted = None,
        }
        let up = self.up(now);
        let show = self
            .wanted
            .filter(|_| visible && up)
            .map(|(detail, _)| detail);
        if show != self.shown {
            layer.show_prepared(floats::STEP_BANNER, show.map(|detail| detail as u64));
            self.shown = show;
        }
        let orb = orb_centre(Side::Opponent, window);
        let below = orb.y + size::ORB_HEIGHT / 2.0 + size::ORB_BEZEL;
        if show.is_some() {
            layer.set_float_look(
                floats::STEP_BANNER,
                FloatLook {
                    point: Vec2::new(window.size.width / 2.0, below + BANNER_GAP),
                    anchor: Vec2::new(0.5, 0.0),
                    ..FloatLook::default()
                },
            );
        }
        let thinking =
            visible && !up && view.is_some_and(|view| matches!(view.status, Status::Thinking));
        if thinking != self.thinking {
            layer.show_prepared(floats::THINKING, thinking.then_some(RING));
            self.thinking = thinking;
        }
        if thinking {
            layer.set_float_look(
                floats::THINKING,
                FloatLook {
                    point: Vec2::new(orb.x, below + THINKING_GAP),
                    anchor: Vec2::new(0.5, 0.0),
                    ..FloatLook::default()
                },
            );
        }
    }
}

/// The banner with the detail at `detail`: the gold title in capitals over the detail, on a
/// smoky shade.
fn banner(detail: usize) -> Element {
    let (title, detail) = words(detail);
    let mut element = column()
        .name("step banner")
        .width(size::BANNER_WIDTH)
        .height(size::BANNER_HEIGHT)
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .gap(crate::tokens::space::M)
        .radius(size::BANNER_HEIGHT / 2.0)
        .background(color::BANNER)
        // It takes no pointer, so the permanents under it stay at hand, as the web's
        // `.step-banner` has `pointer-events: none`.
        .passes(true)
        .child(
            text(&title.to_uppercase())
                .name(title)
                .style(type_style::BANNER_TITLE)
                .color(color::BANNER_TITLE),
        )
        .child(
            text(detail)
                .style(type_style::BANNER_DETAIL)
                .color(color::BANNER_DETAIL),
        );
    element.role = Role::Region;
    element
}

/// The ring under the opponent's orb while they decide, the web's `Thinking` spinner held
/// still: the flat layer draws no arcs, so the whole ring is gold.
fn thinking_ring() -> Element {
    Element::new(Role::Region)
        .name("Opponent thinking")
        .width(size::THINKING)
        .height(size::THINKING)
        .border((color::THINKING, size::THINKING_TRACK))
        .radius(size::THINKING / 2.0)
        .passes(true)
}
