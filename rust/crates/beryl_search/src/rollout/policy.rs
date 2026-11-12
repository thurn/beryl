//! [`Policy`]: one decision's view, and the rules' scores for what it offers.

use std::cell::Cell;

use beryl_engine::card::{
    Ability, AbilityKind, CardDef, CastingOption, Castings, Face, FaceAbility, FaceIx, Keywords,
    ObjectAbility, Players, TargetKind, TargetSpec, TriggerEvent, Types,
};
use beryl_engine::filter::Filter;
use beryl_engine::state::{ObjectFlags, StackEntry};
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{GameTable, Layout, Observation, Opt, Seat, TurnStep, Zone, ZoneKey};

use super::{COUNTER_MIN, Candidate, LAND_MAX, LIBRARY_LOW, REMOVAL_MIN, Rules};
use crate::values::{CardInfo, CardValues};

#[cfg(test)]
mod hold_tests;

#[cfg(test)]
mod waste_tests;

/// One decision's view, for the rules above.
pub(crate) struct Policy<'a, L: Layout> {
    pub(crate) values: &'a CardValues,
    pub(crate) t: &'a GameTable,
    pub(crate) obs: Observation<'a, L>,
    pub(crate) me: Seat,
    /// Whether permanents are valued as the expert values them ([`crate::expert`]).
    pub(crate) expert: bool,
    pub(super) rules: Rules,
    /// [`Policy::may_draw`], once asked: every option of a decision may ask it.
    pub(super) may_draw: Cell<Option<bool>>,
}

impl<L: Layout> Policy<'_, L> {
    pub(crate) fn info(&self, slot: L::Slot) -> Option<CardInfo> {
        self.obs.card(slot).map(|card| self.values.get(card))
    }

    /// What an object is worth: its card's value, plus a creature's power and a
    /// planeswalker's loyalty as they stand on the battlefield, where a land is worth at most
    /// [`LAND_MAX`]. An unknown card is worth 0.
    pub(crate) fn value(&self, slot: L::Slot) -> i32 {
        let Some(info) = self.info(slot) else {
            return 0;
        };
        let mut v = i32::from(info.value);
        let o = self.obs.object(slot);
        if o.zone() == Zone::Battlefield {
            if self.expert
                && let Some(v) = crate::expert::appraise(self, slot, v)
            {
                return v;
            }
            let d = self.obs.derived(slot);
            // A modal double-faced card played as its land face is worth a land.
            if d.types.intersects(Types::LAND) {
                v = v.min(LAND_MAX);
            }
            if d.types.intersects(Types::CREATURE) {
                v += 3 * i32::from(d.power.max(0));
            }
            if d.types.intersects(Types::PLANESWALKER) {
                v += 4 * i32::from(o.counters().loyalty);
            }
        }
        v
    }

    pub(crate) fn is_land(&self, slot: L::Slot) -> bool {
        self.obs.card(slot).is_some_and(|card| {
            self.t
                .def(card)
                .faces
                .iter()
                .any(|f| f.types.intersects(Types::LAND))
        })
    }

    /// What a card in hand is worth keeping: a land is worth much while lands are short and
    /// little once there are six.
    pub(crate) fn in_hand(&self, slot: L::Slot) -> i32 {
        if !self.is_land(slot) {
            return self.value(slot);
        }
        // To the expert, a modal double-faced card with a spell in front is worth the spell
        // at least.
        let spell = self.expert
            && self
                .obs
                .card(slot)
                .is_some_and(|c| !self.t.def(c).front().types.intersects(Types::LAND));
        let mine = |zone| self.obs.zone(ZoneKey::Of(self.me, zone));
        let lands = (mine(Zone::Hand) | mine(Zone::Battlefield))
            .filter(|s| self.is_land(s))
            .count();
        let land = if lands > 5 { 5 } else { 80 };
        if spell {
            land.max(self.value(slot))
        } else {
            land
        }
    }

    /// Keep a hand of two to five lands, and any hand of five cards or fewer, judging the hand
    /// left once a card per mulligan taken goes to the bottom: the least worth keeping, as
    /// the pick for the bottom ([`Purpose::PutOnBottom`]) takes them.
    pub(crate) fn keep(&self) -> bool {
        let mut kept = self.obs.zone(ZoneKey::Of(self.me, Zone::Hand));
        for _ in 0..self.obs.player(self.me).mulligans() {
            // Of the least wanted alike, the first in candidate order goes: the policy's pick.
            let least = self.obs.in_candidate_order(kept);
            match least.min_by_key(|&slot| self.in_hand(slot)) {
                Some(least) => kept.remove(least),
                None => break,
            }
        }
        let lands = kept.filter(|slot| self.is_land(slot)).count();
        kept.count() <= 5 || (2..=5).contains(&lands)
    }

    /// The score of a priority action or a cast offer; 0 is passing.
    pub(crate) fn action(&self, o: Opt<L>) -> i32 {
        if self.rules.hold && self.held(o) {
            return 0;
        }
        match o {
            Opt::PlayLand(slot) => 1000 - self.value(slot),
            Opt::Cast { slot, face, costs } => self.cast(slot, face, costs),
            Opt::Activate { slot, ability } => match ability {
                ObjectAbility::Own(i) => self.activate(slot, i.get()),
                ObjectAbility::Granted(_) => 0,
            },
            _ => 0,
        }
    }

    /// Save instant-speed plays for the opponent's end step, except in windows
    /// where the opportunity could go away or drawing can inform this turn.
    fn held(&self, o: Opt<L>) -> bool {
        let (Opt::Cast { slot, .. } | Opt::Activate { slot, .. }) = o else {
            return false;
        };
        let turn = self.obs.turn();
        let ours = turn.active() == self.me;
        let step = turn.step();
        if step == TurnStep::End
            && (!ours || self.obs.zone(ZoneKey::Of(self.me, Zone::Hand)).count() > 7)
        {
            return false;
        }
        let top = self.obs.stack().last();
        let main = ours
            && top.is_none()
            && matches!(step, TurnStep::PrecombatMain | TurnStep::PostcombatMain);
        let Some(card) = self.obs.card(slot) else {
            return false;
        };
        let face = match o {
            Opt::Cast { face, .. } => face,
            _ => self.obs.object(slot).face(),
        };
        let f = self.t.def(card).face(face);
        let (ability, permanent_spell) = match o {
            Opt::Cast { costs, .. } => {
                if main
                    && !f.types.intersects(Types::INSTANT)
                    && !f.keywords.intersects(Keywords::FLASH)
                {
                    return false;
                }
                (f.spell_ability_with(costs), !f.is_instant_or_sorcery())
            }
            Opt::Activate {
                ability: ObjectAbility::Own(i),
                ..
            } => {
                let a = &f.abilities[i.index()];
                if main && a.cost.loyalty_change().is_some() {
                    return false;
                }
                (Some(a), false)
            }
            // The policy already scores granted abilities as a pass.
            _ => return false,
        };

        if main {
            return !(face == FaceIx::FRONT
                && ability.is_some_and(|a| self.values.get(card).draws(index_of(f, a))));
        }

        let target = ability.and_then(|a| a.targets.first());
        let enters_target = if permanent_spell {
            targeted_enters(f).next().and_then(|a| a.targets.first())
        } else {
            None
        };
        let response_target = if permanent_spell {
            enters_target
        } else {
            target
        };
        if top.is_some_and(|e| e.controller != self.me && e.spell().is_some())
            && response_target.is_some_and(|spec| {
                matches!(spec.kind, TargetKind::Spell | TargetKind::SpellOrPermanent)
            })
        {
            return false;
        }
        if ours {
            let combat = matches!(
                step,
                TurnStep::BeginningOfCombat
                    | TurnStep::DeclareAttackers
                    | TurnStep::DeclareBlockers
                    | TurnStep::FirstStrikeDamage
                    | TurnStep::CombatDamage
                    | TurnStep::EndOfCombat
            );
            !(combat && (target.is_some() || enters_target.is_some()))
        } else {
            !self
                .obs
                .battlefield()
                .iter()
                .any(|&slot| self.flags(slot).has(ObjectFlags::ATTACKING) && self.creature(slot))
        }
    }

    pub(crate) fn cast(&self, slot: L::Slot, face: FaceIx, costs: Castings) -> i32 {
        let Some(card) = self.obs.card(slot) else {
            return 0;
        };
        let info = self.values.get(card);
        let f = self.t.def(card).face(face);
        if info.wheel && self.obs.zone(ZoneKey::Of(self.me, Zone::Hand)).count() > 3 {
            return 0;
        }
        if info.never_cast {
            return 0;
        }
        let drains = info.draws != 0
            && f.abilities
                .iter()
                .enumerate()
                .any(|(i, a)| a.kind == AbilityKind::Triggered && info.draws(i as u8));
        if face == FaceIx::FRONT && drains && !self.may_draw() {
            return 0;
        }
        let mut v = i32::from(info.value);
        let mut evoked = false;
        for option in costs.paid(f) {
            v += match option {
                CastingOption::Kicker { .. } | CastingOption::Offspring(_) => 10,
                CastingOption::Impending { .. } | CastingOption::Overload { .. } => -10,
                // Evoke trades the permanent for its enters ability
                // ([CR 702.74a](https://yawgatog.com/resources/magic-rules/#R70274a)):
                // "When this permanent enters, if its evoke cost was paid, its controller
                // sacrifices it."
                CastingOption::Evoke { .. } => {
                    evoked = true;
                    -30
                }
            };
        }
        // Overloaded, a spell hits each permanent its first target's filter accepts
        // ([CR 702.96a](https://yawgatog.com/resources/magic-rules/#R70296a)): "change its
        // text by replacing all instances of the word 'target' with the word 'each.'"
        let overloaded = costs
            .paid(f)
            .any(|o| matches!(o, CastingOption::Overload { .. }));
        if overloaded
            && let Some(spec) = f.spell_ability().and_then(|a| a.targets.first())
            && self.sweep(spec) < REMOVAL_MIN
        {
            return 0;
        }
        match f.spell_ability_with(costs) {
            Some(a) if face == FaceIx::FRONT && info.draws(index_of(f, a)) && !self.may_draw() => 0,
            Some(a) => self.worth_targeting(a, info.friendly(index_of(f, a)), v, false),
            None => self.enters_worth(f, info, v, self.rules.waste && evoked),
        }
    }

    /// `v` for a permanent spell, or 0 for a noncreature one whose targeted enters trigger
    /// would find nothing worth it: Portable Hole cast onto a board with no nonland permanent of
    /// mana value 2 or less exiles nothing, for the trigger is removed from the stack
    /// ([CR 603.3d](https://yawgatog.com/resources/magic-rules/#R6033d)): "If a choice is required when the
    /// triggered ability goes on the stack but no legal choices can be made for it, ... the
    /// ability is simply removed from the stack." A creature keeps its body unless
    /// the waste rule is pricing an evoke, which must have a worthwhile target.
    fn enters_worth(&self, f: &Face, info: CardInfo, v: i32, require_target: bool) -> i32 {
        if f.types.intersects(Types::CREATURE) && !require_target {
            return v;
        }
        let mut enters = targeted_enters(f).peekable();
        if require_target && enters.peek().is_none() {
            return 0;
        }
        enters.fold(v, |v, a| {
            self.worth_targeting(a, info.friendly(index_of(f, a)), v, require_target)
        })
    }

    pub(crate) fn activate(&self, slot: L::Slot, i: u8) -> i32 {
        let Some(card) = self.obs.card(slot) else {
            return 0;
        };
        let info = self.values.get(card);
        if info.never(i) {
            return 0;
        }
        let o = self.obs.object(slot);
        if o.face() == FaceIx::FRONT && info.draws(i) && !self.may_draw() {
            return 0;
        }
        if o.face() == FaceIx::FRONT && info.empties(i) {
            return if self.obs.library(self.me).len() <= 7 {
                1000
            } else {
                0
            };
        }
        let a = &self.t.def(card).face(o.face()).abilities[usize::from(i)];
        let v = match a.cost.loyalty_change() {
            Some(l) if l >= 0 => 20,
            Some(l) if l <= -8 => 200,
            Some(l) => 20 + 5 * i32::from(-l),
            None if a.cost.does(beryl_engine::card::CostAction::Discard) => 3,
            None => 15,
        };
        self.worth_targeting(a, info.friendly(i), v, false)
    }

    /// Whether drawing by choice is safe: our library holds more than [`LIBRARY_LOW`] cards,
    /// we hold a wheel to refill it, or a permanent of ours wins us the game instead of the
    /// draw from an empty library that would lose it
    /// ([CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b)): "If a player
    /// attempted to draw a card from a library with no cards in it since the last time
    /// state-based actions were checked, that player loses the game."
    pub(crate) fn may_draw(&self) -> bool {
        if let Some(may) = self.may_draw.get() {
            return may;
        }
        let may = self.find_may_draw();
        self.may_draw.set(Some(may));
        may
    }

    fn find_may_draw(&self) -> bool {
        self.obs.library(self.me).len() > LIBRARY_LOW
            || self
                .obs
                .zone(ZoneKey::Of(self.me, Zone::Hand))
                .any(|s| self.info(s).is_some_and(|info| info.wheel))
            || self.wins_from_empty(self.me)
    }

    /// Whether `who` controls a permanent that wins them the game instead of the draw from an
    /// empty library, as Jace, Wielder of Mysteries: "If you would draw a card while your
    /// library has no cards in it, you win the game instead."
    pub(crate) fn wins_from_empty(&self, who: Seat) -> bool {
        self.obs.battlefield().iter().any(|&slot| {
            self.obs.controller(slot) == who
                && self.obs.card(slot).is_some_and(|card| {
                    self.values.get(card).wins_instead
                        && self
                            .t
                            .def(card)
                            .face(self.obs.object(slot).face())
                            .abilities
                            .iter()
                            .any(beryl_engine::wins_instead_of_drawing)
                })
        })
    }

    /// `v`, or 0 when the ability's first target is not worth it: a harmful one needs an
    /// opponent's object worth the removal or counter threshold, unless it may target
    /// nothing and `require_target` is false. A helpful one needs one of ours.
    fn worth_targeting(&self, a: &Ability, friendly: bool, v: i32, require_target: bool) -> i32 {
        let Some(spec) = a.targets.first() else {
            return v;
        };
        let friendly = friendly || matches!(whose(&spec.filter), Some(Players::You));
        let min = match spec.kind {
            _ if friendly => 0,
            TargetKind::Spell => COUNTER_MIN,
            TargetKind::Player => 0,
            _ => REMOVAL_MIN,
        };
        // The best target is worth `min` exactly when some target is.
        if self.target_values(spec, friendly).any(|best| best >= min) {
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
        self.target_values(spec, friendly).max()
    }

    /// What each object a spec could target on the side it should is worth, as
    /// [`Policy::best_target`] weighs them, players as 0.
    fn target_values<'s>(
        &'s self,
        spec: &'s TargetSpec,
        friendly: bool,
    ) -> impl Iterator<Item = i32> + 's {
        let side = if friendly {
            self.me
        } else {
            self.me.opponent()
        };
        let ok = move |slot: L::Slot, controller: Seat| {
            controller == side && self.obs.matches(self.t, &spec.filter, slot)
        };
        let permanents = matches!(
            spec.kind,
            TargetKind::Permanent | TargetKind::SpellOrPermanent
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
            .filter_map(|e| e.spell().map(|r| (r.slot, e.controller)))
            .filter(move |&(slot, controller)| ok(slot, controller))
            .map(|(slot, _)| self.value(slot));
        let graveyard = if graveyard {
            self.obs.graveyard(side)
        } else {
            &[]
        }
        .iter()
        .filter(|&&slot| self.obs.matches(self.t, &spec.filter, slot))
        .map(|&slot| self.value(slot));
        let player = (spec.kind == TargetKind::Player).then_some(0);
        on_battlefield
            .chain(on_stack)
            .chain(graveyard)
            .chain(player)
    }

    /// What hitting each permanent `spec`'s filter accepts gains a harmful effect: the value of
    /// the opponent's, less that of ours.
    pub(crate) fn sweep(&self, spec: &TargetSpec) -> i32 {
        self.obs
            .battlefield()
            .iter()
            .filter(|&&slot| self.obs.matches(self.t, &spec.filter, slot))
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
    pub(crate) fn targeting(&self) -> Option<(&'static TargetSpec, bool)> {
        let e: &StackEntry<L> = self.obs.stack().last()?;
        let def: &'static CardDef = self.t.def(e.card);
        let face: &'static Face = def.face(e.face);
        let a = match e.ability {
            Some(ability) => face.ability(ability),
            None => face.spell_ability_with(e.costs)?,
        };
        let spec = a.targets.first()?;
        let index = match e.ability {
            Some(FaceAbility::Own(i)) => i.get(),
            Some(FaceAbility::Grant(_)) => u8::MAX,
            None => index_of(face, a),
        };
        let friendly = self.values.get(e.card).friendly(index)
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
                    let controller = if matches!(o.zone(), Zone::Battlefield | Zone::Stack) {
                        self.obs.controller(slot)
                    } else {
                        o.owner()
                    };
                    (controller, self.value(slot))
                }
                None => return 0,
            },
        };
        if controller == side { 1000 + v } else { -v }
    }

    pub(crate) fn creature(&self, slot: L::Slot) -> bool {
        self.obs.derived(slot).types.intersects(Types::CREATURE)
    }

    pub(crate) fn flags(&self, slot: L::Slot) -> ObjectFlags {
        self.obs.object(slot).flags()
    }

    /// Whether `blocker` could block `attacker`, by evasion
    /// ([CR 702.9b](https://yawgatog.com/resources/magic-rules/#R7029b)): "A creature with
    /// flying can't be blocked except by creatures with flying and/or reach."
    pub(crate) fn evades(&self, attacker: L::Slot, blocker: L::Slot) -> bool {
        let has = |slot: L::Slot, k: Keywords| self.obs.derived(slot).keywords.intersects(k);
        has(attacker, Keywords::FLYING) && !has(blocker, Keywords::FLYING.union(Keywords::REACH))
    }

    /// Whether `by`'s power is at least the damage left to kill `slot`.
    pub(crate) fn kills(&self, by: L::Slot, slot: L::Slot) -> bool {
        let left =
            i16::from(self.obs.derived(slot).toughness) - i16::from(self.obs.object(slot).damage());
        i16::from(self.obs.derived(by).power) >= left
    }

    /// No untapped creature of the opponent that could block `attacker` would kill it, and
    /// it deals damage.
    pub(crate) fn safe_attack(&self, attacker: L::Slot) -> bool {
        self.obs.derived(attacker).power > 0
            && !self.obs.battlefield().iter().any(|&b| {
                self.obs.controller(b) != self.me
                    && self.creature(b)
                    && !self.flags(b).has(ObjectFlags::TAPPED)
                    && !self.evades(attacker, b)
                    && self.kills(b, attacker)
            })
    }

    /// The score of attacking `walker`, against 0 for the player: its value, unless the
    /// attackers already declared at it deal all its loyalty. Damage dealt to a planeswalker
    /// removes that much loyalty
    /// ([CR 120.3c](https://yawgatog.com/resources/magic-rules/#R1203c)): "Damage dealt to
    /// a planeswalker causes that many loyalty counters to be removed from that
    /// planeswalker."
    pub(crate) fn walker_target(&self, walker: L::Slot) -> i32 {
        let sent: i32 = self
            .obs
            .battlefield()
            .iter()
            .filter(|&&a| {
                let o = self.obs.object(a);
                self.obs.controller(a) == self.me
                    && o.flags().has(ObjectFlags::ATTACKING)
                    && o.combat() == Some(walker)
            })
            .map(|&a| i32::from(self.obs.derived(a).power.max(0)))
            .sum();
        if sent < i32::from(self.obs.object(walker).counters().loyalty) {
            self.value(walker)
        } else {
            -1
        }
    }

    pub(crate) fn attacking(&self, slot: L::Slot) -> bool {
        self.obs.controller(slot) != self.me && self.flags(slot).has(ObjectFlags::ATTACKING)
    }

    pub(crate) fn blocked(&self, attacker: L::Slot) -> bool {
        self.obs.battlefield().iter().any(|&b| {
            let o = self.obs.object(b);
            o.flags().has(ObjectFlags::BLOCKING) && o.combat() == Some(attacker)
        })
    }

    /// The damage the attackers not yet blocked would deal us.
    pub(crate) fn unblocked_damage(&self) -> i32 {
        self.obs
            .battlefield()
            .iter()
            .filter(|&&a| {
                self.attacking(a)
                    && !self.flags(a).has(ObjectFlags::ATTACKS_PLANESWALKER)
                    && !self.blocked(a)
            })
            .map(|&a| i32::from(self.obs.derived(a).power.max(0)))
            .sum()
    }

    /// The score of `blocker` blocking `attacker`, if it is a block worth making: killing it
    /// and surviving, trading up, surviving, or chumping when the damage would be lethal.
    pub(crate) fn block(&self, blocker: L::Slot, attacker: L::Slot) -> Option<i32> {
        if !self.attacking(attacker) || self.blocked(attacker) || self.evades(attacker, blocker) {
            return None;
        }
        let (kill, die) = (self.kills(blocker, attacker), self.kills(attacker, blocker));
        let (va, vb) = (self.value(attacker), self.value(blocker));
        let power = i32::from(self.obs.derived(attacker).power.max(0));
        match (kill, die) {
            (true, false) => Some(300 + va),
            (true, true) if va >= vb => Some(200 + va - vb),
            (false, false) => Some(100 + power),
            _ if self.unblocked_damage() >= i32::from(self.obs.player(self.me).life())
                && !self.flags(attacker).has(ObjectFlags::ATTACKS_PLANESWALKER) =>
            {
                // Above declining (0) at any blocker value, the cheapest blocker first.
                Some(20 + power - vb / 10)
            }
            _ => None,
        }
    }

    pub(crate) fn best_block(&self, blocker: L::Slot) -> Option<i32> {
        self.obs
            .battlefield()
            .iter()
            .filter_map(|&a| self.block(blocker, a))
            .max()
    }

    /// The creature just declared as a blocker, whose attacker is being chosen: ours,
    /// blocking, and blocking nothing yet (CR 509.1a).
    pub(crate) fn declaring_blocker(&self) -> Option<L::Slot> {
        self.obs.battlefield().iter().copied().find(|&b| {
            let o = self.obs.object(b);
            self.obs.controller(b) == self.me
                && o.flags().has(ObjectFlags::BLOCKING)
                && o.combat().is_none()
        })
    }
}

fn targeted_enters(face: &Face) -> impl Iterator<Item = &'static Ability> {
    face.abilities.iter().filter(|a| {
        a.kind == AbilityKind::Triggered
            && a.trigger.is_some_and(|t| t.is_this(TriggerEvent::Enters))
            && !a.targets.is_empty()
    })
}

/// Whose objects a filter requires, if it says.
pub(crate) fn whose(f: &Filter) -> Option<&'static Players> {
    match f {
        Filter::Whose(players) => Some(players),
        Filter::All(fs) => fs.iter().find_map(whose),
        _ => None,
    }
}

/// The index of one of a face's abilities.
pub(crate) fn index_of(face: &Face, a: &Ability) -> u8 {
    face.abilities
        .iter()
        .position(|b| std::ptr::eq(a, b))
        .map_or(u8::MAX, |i| i as u8)
}
