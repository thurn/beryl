//! The rollout policy's judgement of targets: whether a targeted ability has a target worth
//! it, and which candidate target it prefers.

use beryl_engine::card::{
    Ability, CardDef, Face, FaceAbility, Keywords, Players, TargetKind, TargetSpec, Types,
};
use beryl_engine::state::StackEntry;
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{Layout, Opt, Seat, TurnStep, Zone};

use super::Candidate;
use super::policy::{Policy, index_of, whose};

impl<L: Layout> Policy<'_, L> {
    /// `v`, or 0 when the ability's first target is not worth it: a harmful one needs an
    /// opponent's object worth the removal or counter threshold, unless it may target
    /// nothing and `require_target` is false. A helpful one needs one of ours.
    ///
    /// A spell that only `bounces` its target back to its owner's hand is worth nothing aimed
    /// at a spell its owner can cast again at once ([`Policy::recastable`]): it costs them
    /// only mana they have spare.
    pub(super) fn worth_targeting(
        &self,
        a: &Ability,
        friendly: bool,
        bounces: bool,
        v: i32,
        require_target: bool,
    ) -> i32 {
        let Some(spec) = a.targets().first() else {
            return v;
        };
        let friendly = friendly || matches!(whose(&spec.filter), Some(Players::You));
        let min = match spec.kind {
            _ if friendly => 0,
            TargetKind::Spell => self.weights.counter_min,
            // Damage to an opponent is always worth something, as a player counts 0.
            TargetKind::Player | TargetKind::PermanentOrPlayer => 0,
            _ => self.weights.removal_min,
        };
        // The best target is worth `min` exactly when some target is.
        if self
            .target_values(spec, friendly, bounces)
            .any(|best| best >= min)
        {
            v
        } else if !require_target && spec.optional && !matches!(spec.kind, TargetKind::Spell) {
            v / 2
        } else {
            0
        }
    }

    /// The most valuable object a spec could target on the side it should: the opponent's
    /// for a harmful effect, ours for a helpful one. Players count as 0.
    pub(crate) fn best_target(&self, spec: &TargetSpec, friendly: bool) -> Option<i32> {
        self.target_values(spec, friendly, false).max()
    }

    /// What each object a spec could target on the side it should is worth, as
    /// [`Policy::best_target`] weighs them, players as 0.
    fn target_values<'s>(
        &'s self,
        spec: &'s TargetSpec,
        friendly: bool,
        bounces: bool,
    ) -> impl Iterator<Item = i32> + 's {
        let side = if friendly {
            self.me
        } else {
            self.me.opponent()
        };
        // A guess at the best target reads the spec as printed: it has no source at hand.
        let printed = &beryl_engine::card::Words::NONE;
        let ok = move |slot: L::Slot, controller: Seat| {
            controller == side && self.obs.matches(self.t, &spec.filter, slot, printed)
        };
        let permanents = matches!(
            spec.kind,
            TargetKind::Permanent | TargetKind::SpellOrPermanent | TargetKind::PermanentOrPlayer
        );
        let spells = matches!(spec.kind, TargetKind::Spell | TargetKind::SpellOrPermanent);
        let graveyard = spec.kind == TargetKind::GraveyardCard;
        let on_battlefield = if permanents {
            self.obs.battlefield()
        } else {
            &[]
        }
        .iter()
        .filter(move |&&slot| ok(slot, self.obs.controller(slot)))
        .map(|&slot| self.value(slot));
        let on_stack = if spells { self.obs.stack() } else { &[] }
            .iter()
            .filter_map(|e| e.spell().map(|r| (r.slot, e.controller())))
            .filter(move |&(slot, controller)| {
                ok(slot, controller) && !(bounces && self.recastable(slot, controller))
            })
            .map(|(slot, _)| self.value(slot));
        let graveyard = if graveyard {
            self.obs.graveyard(side)
        } else {
            &[]
        }
        .iter()
        .filter(|&&slot| self.obs.matches(self.t, &spec.filter, slot, printed))
        .map(|&slot| self.value(slot));
        let player = matches!(
            spec.kind,
            TargetKind::Player | TargetKind::PermanentOrPlayer
        )
        .then_some(0);
        on_battlefield
            .chain(on_stack)
            .chain(graveyard)
            .chain(player)
    }

    /// What hitting each permanent `spec`'s filter accepts gains a harmful effect: the value of
    /// the opponent's, less that of ours.
    pub(crate) fn sweep(&self, spec: &TargetSpec) -> i32 {
        // As in `target_values`, the spec is read as printed.
        let printed = &beryl_engine::card::Words::NONE;
        self.obs
            .battlefield()
            .iter()
            .filter(|&&slot| self.obs.matches(self.t, &spec.filter, slot, printed))
            .map(|&slot| {
                if self.obs.controller(slot) == self.me {
                    -self.value(slot)
                } else {
                    self.value(slot)
                }
            })
            .sum()
    }

    /// The spec being targeted for and whether its effect helps its controller: the first
    /// spec of the spell or ability on top of the stack, which a casting or a triggered
    /// ability being put on the stack puts there first (CR 601.2a).
    #[allow(
        clippy::disallowed_methods,
        reason = "reads the face of the top stack entry"
    )]
    pub(crate) fn targeting(&self) -> Option<(&'static TargetSpec, bool)> {
        let e: &StackEntry<L> = self.obs.stack().last()?;
        let def: &'static CardDef = self.t.def(e.card());
        let face: &'static Face = def.face(e.face());
        let a = match e.ability() {
            Some(ability) => face.ability(ability),
            None => face.spell_ability_with(e.costs())?,
        };
        let spec = a.targets().first()?;
        let index = match e.ability() {
            Some(FaceAbility::Own(i)) => i.get(),
            Some(FaceAbility::Grant(_)) => u8::MAX,
            None => index_of(face, a),
        };
        let friendly = self.values.get(e.card()).friendly(index)
            || matches!(whose(&spec.filter), Some(Players::You));
        Some((spec, friendly))
    }

    /// A target's score: a candidate on the right side by its value, far above the wrong
    /// side; choosing none in between.
    pub(crate) fn target(&self, c: Candidate<L>, friendly: bool) -> i32 {
        let side = if friendly {
            self.me
        } else {
            self.me.opponent()
        };
        let (controller, v) = match c {
            Candidate::Option(Opt::Player(p)) => (p, 0),
            Candidate::Option(Opt::Done) => return 500,
            _ => match c.object() {
                Some(slot) => {
                    let o = self.obs.object(slot);
                    // A card in a shared library or graveyard has no owner: it is in both
                    // players' zones, so it counts as on the wanted side.
                    let controller = if matches!(o.zone(), Zone::Battlefield | Zone::Stack) {
                        self.obs.controller(slot)
                    } else {
                        o.owner().unwrap_or(side)
                    };
                    (controller, self.value(slot))
                }
                None => return 0,
            },
        };
        if controller == side { 1000 + v } else { -v }
    }

    /// Whether `controller` could cast the spell `slot` again this turn if it were returned
    /// to their hand: it is an instant or has flash, or it is their own main phase, and they
    /// have an untapped mana source for each point of its mana value. Returning a spell to
    /// its owner's hand does not counter it, so they lose only the mana they paid.
    #[allow(
        clippy::disallowed_methods,
        reason = "reads the face the spell was cast as"
    )]
    pub(super) fn recastable(&self, slot: L::Slot, controller: Seat) -> bool {
        let Some(card) = self.obs.card(slot) else {
            return false;
        };
        let f = self.t.def(card).face(self.obs.object(slot).face());
        let turn = self.obs.turn();
        let timing = f.types.intersects(Types::INSTANT)
            || f.keywords.intersects(Keywords::FLASH)
            || (turn.active() == controller
                && matches!(
                    turn.step(),
                    TurnStep::PrecombatMain | TurnStep::PostcombatMain
                ));
        timing && self.untapped_mana_sources(controller) >= usize::from(f.cost.mana_value())
    }
}
