//! The marks a permanent's own record calls for, drawn as quads over its card
//! (docs/beryl-3d.md, "Objects and placement": marks are separate quads). They say what the
//! record says, so they need no span and no step: each is drawn while the record the table
//! draws the card from says so, and changes as the step that carries a new record begins.
//!
//! - Summoning sickness: a cyan crescent on a dark badge, like the web's `.card-sick-moon`.
//!   The badge sits over the art to leave the card's text and stats visible.
//! - A token: a coin at the bottom left, where the web's `.card-counters` row puts its label.
//! - A permanent its owner does not control gets no mark. MTG Arena draws none: a Coveted Jewel
//!   the player has taken shows nothing when tapped and only the pale "playable" outline the
//!   player's lands also show when it may be used (`0VsYGs88NXA` at 29:20 and 30:32).
//! - A card the hand shows from its owner's graveyard, one they may cast from there: two soft
//!   halos, violet and pale, breathing out of step, as MTG Arena rings such a card in drifting
//!   smoke (`tovCPHl-gOM` at 9:08 and 15:02.5). Drawn on the card wherever it is, so the ring
//!   rides with it from the graveyard into the hand.
//! - An attacker: a wedge along the edge facing the defender. It turns gold as the creature
//!   becomes blocked (Arena, `IEXcjBn3BXk` at 22:23). In the Enhanced tier, this is drawn as a
//!   procedural plasma chevron (crimson-orange unblocked, solar-gold when blocked) with looping
//!   ember sparks.
//! - A blocker: a gold crescent along the edge facing the attacker, opening toward the card. In
//!   the Enhanced tier, this is drawn as a procedural golden energy shield dome.
//!
//! The wedge and the crescent fade in over `fade_ms` from the frame the record first says the
//! creature fights. The wedge starts a little large and settles over `pop_ms`, and the crescent
//! opens from `opens_from` of its size over `open_ms`, as in `fighterLight`. The web keeps
//! these times in its code rather than in `data/ui-conventions.json`, so they are under `own`
//! in `data/client/own.json` with the sizes and offsets, which are shares of a card.

use std::cell::Cell;
use std::f32::consts::PI;

use beryl_protocol::protocol::{Combat, ObjectView, Side};
use glam::{Affine3A, Quat, Vec2};

use super::Look;
use kit_base::atlas::Tier;
use kit_base::data::light::Own;
use kit_base::tokens::{badge, color};
use kit_frame::frame::{Band, Draw, FrameBuilder, PaintBand, Sprite, sprites};
use kit_fx::effects::edge::{Edge, past_edge, with_opacity, write_away_halo};
use kit_fx::effects::vfx::{FighterKind, FighterVfx, write_fighter};

/// How a creature fights, as its record says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Fighting {
    #[default]
    No,
    Attacks {
        blocked: bool,
    },
    Blocks,
}

impl Fighting {
    fn of(record: &ObjectView) -> Self {
        match record.combat {
            Some(Combat::Attacking { blocked, .. }) => Self::Attacks { blocked },
            Some(Combat::Blocking { .. }) => Self::Blocks,
            None => Self::No,
        }
    }

    fn same_role(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::No, Self::No)
                | (Self::Attacks { .. }, Self::Attacks { .. })
                | (Self::Blocks, Self::Blocks)
        )
    }
}

/// For each slot, how its creature fought in the last frame that drew it and since when, in
/// nanoseconds since the client started. Drawing reads the table without changing it, so the
/// times are kept in cells. They are bookkeeping, not what the table shows, so two tables are
/// equal whatever their times.
#[derive(Clone, Debug)]
pub(super) struct Fights(Vec<Cell<(Fighting, u64)>>);

impl Fights {
    /// Room for `slots` slots, so that no frame allocates.
    pub(super) fn with_capacity(slots: usize) -> Self {
        Self(vec![Cell::new((Fighting::No, 0)); slots])
    }

    /// Since when the creature in `slot` has fought as `fighting`, noting `now` if it did not
    /// in the frame before. Transitions between unblocked and blocked attacking keep the original
    /// attack start time so the chevron does not replay its pop animation. A slot past the room
    /// reserved is taken to have just begun.
    fn since(&self, slot: usize, fighting: Fighting, now: u64) -> u64 {
        let Some(cell) = self.0.get(slot) else {
            return now;
        };
        let (was, since) = cell.get();
        if was.same_role(fighting) {
            if was != fighting {
                cell.set((fighting, since));
            }
            return since;
        }
        cell.set((fighting, now));
        now
    }

    /// The slot holds no creature drawn in this frame.
    pub(super) fn forget(&self, slot: usize) {
        if let Some(cell) = self.0.get(slot) {
            cell.set((Fighting::No, 0));
        }
    }
}

impl PartialEq for Fights {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// Writes the marks of `record`, the permanent in `slot` drawn as `look`, at `now`.
pub(super) fn write(
    (slot, record): (usize, &ObjectView),
    (look, own): (&Look, &Own),
    fights: &Fights,
    (now, band): (u64, PaintBand),
    builder: &mut FrameBuilder,
) {
    let fighting = Fighting::of(record);
    let since = fights.since(slot, fighting, now);
    if look.pose.opacity <= 0.0 {
        return;
    }
    let card = builder.card();
    let mut marks = Marks::on(look, own, band, builder);
    if record.summoning_sick {
        marks.moon(own.moon.at * card, own.moon.size * card.x);
    }
    if record.token {
        let size = Vec2::splat(own.coin.size * card.x);
        marks.icon(sprites::COIN, own.coin.at * card, size, color::COIN);
    }
    if fighting != Fighting::No {
        #[expect(
            clippy::cast_precision_loss,
            reason = "an age in nanoseconds; a few seconds fit an f32's precision well enough"
        )]
        let age = now.saturating_sub(since) as f32 / 1e6;
        #[expect(
            clippy::cast_precision_loss,
            reason = "wrapped animation clock in seconds fits an f32's precision"
        )]
        let time_s = (now % 1_000_000_000_000) as f32 / 1e9;
        let edge = Edge::toward(look.pose.rotation, defending(record.controller), card);
        marks.fight(slot, fighting, &edge, age, time_s);
    }
}

/// Writes the halo of a card the hand shows from its owner's graveyard, drawn as `look`, at
/// `now`, in nanoseconds since the client started.
pub(super) fn from_graveyard(
    (look, own): (&Look, &Own),
    (now, band): (u64, PaintBand),
    builder: &mut FrameBuilder,
) {
    if look.pose.opacity > 0.0 {
        let place = (look.pose.upward(), own.lift);
        let opacity = look.pose.opacity.clamp(0.0, 1.0);
        write_away_halo(
            builder,
            &own.away,
            place,
            (now, band.records(Band::Glows), opacity),
        );
    }
}

/// Writes the marks on one card: quads in the card's own space, lying just above its upper
/// side whether its face is up or down, in the band of marks on cards on the table or on cards
/// above it, as clear as the card is.
struct Marks<'b, 'f> {
    own: &'b Own,
    card: Affine3A,
    band: Band,
    glow_band: Band,
    particle_band: Band,
    opacity: f32,
    builder: &'b mut FrameBuilder<'f>,
}

impl<'b, 'f> Marks<'b, 'f> {
    fn on(look: &Look, own: &'b Own, band: PaintBand, builder: &'b mut FrameBuilder<'f>) -> Self {
        let pose = look.pose;
        Self {
            own,
            card: pose.upward(),
            band: band.records(Band::TableMarks),
            glow_band: band.records(Band::Glows),
            particle_band: band.records(Band::Particles),
            opacity: pose.opacity.clamp(0.0, 1.0),
            builder,
        }
    }

    /// The quad `size` large with its middle at `at`, turned by `turn` about the card's normal.
    fn place(&self, at: Vec2, size: Vec2, turn: f32) -> Affine3A {
        self.card
            * Affine3A::from_scale_rotation_translation(
                size.extend(1.0),
                Quat::from_rotation_z(turn),
                at.extend(self.own.lift),
            )
    }

    /// The quad `size` large with its middle `out` past the middle of `edge`, turned by `turn`
    /// about the card's normal.
    fn past(&self, edge: &Edge, out: f32, size: Vec2, turn: f32) -> Affine3A {
        past_edge(self.card, self.own.lift, edge, out, size, turn)
    }

    fn moon(&mut self, at: Vec2, diameter: f32) {
        let badge_shares = self.own.moon_badge;
        let size = Vec2::splat(diameter);
        // Keep the backdrop between the card face and the crescent in the depth sort.
        let mut panel = self.place(at, size, 0.0);
        panel.translation -= self.card.matrix3.z_axis * (self.own.lift * 0.5);
        self.builder.draw(
            self.band,
            Draw::panel(
                panel,
                with_opacity(badge::COUNTER, self.opacity),
                with_opacity(color::METAL_BRIGHT, self.opacity),
                size,
                diameter / 2.0,
                diameter * badge_shares.border,
            ),
        );
        self.icon(sprites::MOON, at, size * badge_shares.icon, color::MOON);
    }

    fn icon(&mut self, sprite: Sprite, at: Vec2, size: Vec2, tint: [u8; 4]) {
        let draw = Draw::icon(
            self.place(at, size, 0.0),
            with_opacity(tint, self.opacity),
            sprite,
        );
        self.builder.draw(self.band, draw);
    }

    /// The wedge of an attacker or the crescent of a blocker on `edge`, `age` milliseconds
    /// after its record first said it fights. A block changes the attacker's color without
    /// replaying its entrance. Baseline draws the lightweight atlas sprites; Enhanced draws the
    /// procedural plasma chevron or golden shield dome in `Band::Glows` (for bloom) plus looping
    /// ember sparks in `Band::Particles`.
    fn fight(&mut self, slot: usize, fighting: Fighting, edge: &Edge, age: f32, time_s: f32) {
        let fade = (age / self.own.fade_ms).clamp(0.0, 1.0);
        let turn = edge.axis.y.atan2(edge.axis.x) - PI / 2.0;
        if self.builder.tier() == Tier::Enhanced {
            self.fight_enhanced(slot, fighting, edge, (age, time_s, fade, turn));
            return;
        }
        if let Fighting::Attacks { blocked } = fighting {
            let pop = (-age / self.own.wedge.pop_ms).exp();
            let card_width = self.builder.card().x;
            let length = self.own.wedge.chevron * card_width * (1.0 + self.own.wedge.pop.y * pop);
            let size = sprites::wedge_size(
                edge.across * (1.0 + self.own.wedge.pop.x * pop),
                length,
                card_width,
            ) * self.own.wedge.scale;
            // Scale the halo as well as the wedge so it leaves room for the opposing crescent.
            let transform = self.past(edge, length * self.own.wedge.scale.y / 2.0, size, turn);
            let (glow, core) = if blocked {
                (color::BLOCK_LINE, color::BLOCK_LINE_CORE)
            } else {
                (color::WEDGE, color::WEDGE_HOT)
            };
            for (sprite, tint) in [(sprites::WEDGE, glow), (sprites::WEDGE_HOT, core)] {
                let tint = with_opacity(tint, fade * self.opacity);
                self.builder
                    .draw(self.band, Draw::glow(transform, tint, sprite, 0.0));
            }
            return;
        }
        let t = (age / self.own.crescent.open_ms).clamp(0.0, 1.0);
        let open = self.own.crescent.opens_from
            + (1.0 - self.own.crescent.opens_from) * (1.0 - (1.0 - t).powi(3));
        let card_width = self.builder.card().x;
        let size = self.own.crescent.size * Vec2::new(edge.across, card_width) * open;
        let out = self.own.crescent.gap * card_width;
        // The sprite opens along +x. Turn its opening back toward the blocker.
        let draw = Draw::glow(
            self.past(edge, out, Vec2::new(size.y, size.x), turn - PI / 2.0),
            with_opacity(color::CRESCENT, fade * self.opacity),
            sprites::CRESCENT,
            0.0,
        );
        self.builder.draw(self.band, draw);
    }

    /// Enhanced-tier procedural attacker chevron (unblocked crimson-ember / blocked solar-gold)
    /// and blocker golden shield dome, plus deterministic ember sparks.
    fn fight_enhanced(
        &mut self,
        slot: usize,
        fighting: Fighting,
        edge: &Edge,
        (age, time_s, fade, turn): (f32, f32, f32, f32),
    ) {
        let kind = match fighting {
            Fighting::Attacks { blocked } => FighterKind::Attacker { blocked },
            Fighting::Blocks => FighterKind::Blocker,
            Fighting::No => return,
        };
        let card = self.card;
        let lift = self.own.lift;
        let fx = FighterVfx {
            slot,
            kind,
            glow_band: self.glow_band,
            particle_band: self.particle_band,
            across: edge.across,
            age_ms: age,
            time_s,
            opacity: fade * self.opacity,
        };
        write_fighter(self.builder, self.own, &fx, |out, size| {
            past_edge(card, lift, edge, out, size, turn)
        });
    }
}

/// The way across the table, in the table's space, that a creature controlled by `controller`
/// faces as it fights: an attacker toward the players it attacks, and a blocker toward the
/// attacker it blocks, so both toward their controller's opponent. The viewer sits at -y.
fn defending(controller: Side) -> Vec2 {
    match controller {
        Side::You => Vec2::Y,
        Side::Opponent => -Vec2::Y,
    }
}
