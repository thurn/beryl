//! Setup and legal game actions for each event recipe.

use beryl_card::{
    Behavior, CardDef, CastingOption, Castings, CounterKind, Effect, FaceIx, Filter, Seat,
    TargetKind, Types,
};
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, P0, P1, Permanent, Scenario};

use super::{Recipe, players};
use crate::derive::play::Stimulus;
use crate::derive::position::{LAST_TURN, Place, TURN, preamble, printed, start};
use crate::{CARDS, names, props};

pub(in crate::derive) struct Opening {
    pub(in crate::derive) label: String,
    pub(in crate::derive) start: Scenario,
    pub(in crate::derive) place: Place,
    pub(in crate::derive) stimuli: Vec<Stimulus>,
}

impl Opening {
    fn new(def: &'static CardDef, place: Place, label: impl Into<String>) -> Self {
        Self {
            label: format!("{} {}", label.into(), place.words()),
            start: start(def, place, None),
            place,
            stimuli: preamble(def, place),
        }
    }

    fn hand(&mut self, player: Seat, card: &'static str) {
        let setup = &mut self.start.setup.players[player.index()];
        if !setup.hand.contains(&card) {
            setup.hand = Vec::leak([setup.hand, &[card]].concat());
        }
    }

    fn graveyard_target(&mut self, def: &CardDef, filter: Filter) {
        let eligible = |name: &str, player| {
            CARDS
                .get(name)
                .is_some_and(|card| matches(filter, card, player, Zone::Graveyard))
        };
        let players = &mut self.start.setup.players;
        if Seat::BOTH.into_iter().any(|player| {
            players[player.index()]
                .graveyard
                .iter()
                .any(|card| eligible(card, player))
        }) {
            return;
        }
        let candidates = || {
            props::ALL
                .iter()
                .filter_map(|prop| CARDS.get(prop.card))
                .chain(CARDS.cards().iter().copied())
        };
        // Put the target across the table when the filter permits either graveyard.
        for player in [P1, P0] {
            if let Some(card) = candidates().find(|card| {
                card.name != def.name && matches(filter, card, player, Zone::Graveyard)
            }) {
                let setup = &mut players[player.index()];
                setup.graveyard = Vec::leak([setup.graveyard, &[card.name]].concat());
                self.admit(def, card.name);
                break;
            }
        }
    }

    fn permanent(&mut self, player: Seat, card: &'static str) {
        let setup = &mut self.start.setup.players[player.index()];
        setup.permanents = Vec::leak([setup.permanents, &[Permanent::new(card)]].concat());
    }

    fn admit(&mut self, def: &CardDef, card: &'static str) {
        if card != def.name && !props::ALL.iter().any(|prop| prop.card == card) {
            self.start.interaction = Vec::leak([self.start.interaction, &[card]].concat());
        }
    }

    fn own_turn(&mut self, player: Seat) {
        if player == P1 {
            self.stimuli
                .push(Stimulus::PassUntil(LAST_TURN, TurnStep::PrecombatMain));
        }
    }

    fn cast(&mut self, def: &CardDef, player: Seat, card: &'static str) {
        self.hand(player, card);
        self.admit(def, card);
        self.stimuli.push(Stimulus::Try(Action::cast(player, card)));
    }

    fn aim(&mut self, player: Seat, card: &'static str) {
        self.stimuli.push(Stimulus::Aim(Action::choose(
            player,
            DecisionKind::Target,
            card,
        )));
    }
}

/// Match printed characteristics in the zone where the planned event will happen. The engine
/// still makes the real trigger and replacement checks after continuous effects.
fn matches(filter: Filter, def: &CardDef, owner: Seat, zone: Zone) -> bool {
    match filter {
        Filter::Whose(whose) | Filter::Owned(whose) => players(*whose).contains(&owner),
        Filter::Not(inner) => !matches(*inner, def, owner, zone),
        Filter::All(filters) => filters.iter().all(|&f| matches(f, def, owner, zone)),
        Filter::AnyOf(filters) => filters.iter().any(|&f| matches(f, def, owner, zone)),
        part => printed(part, Permanent::new(def.name), def.front(), zone).unwrap_or(false),
    }
}

fn cast_partner(def: &CardDef, player: Seat, filter: Filter, zone: Zone) -> Option<&'static str> {
    // Try the simple props first. Use the implemented pool for a printed restriction none
    // of them fits, such as legendary. Keep the first match so a new card does not add runs.
    let practice = [
        props::VANILLA,
        props::SORCERY,
        props::REGENERATION,
        props::EQUIPMENT,
        props::FLIER,
    ];
    practice
        .into_iter()
        .filter_map(|prop| CARDS.get(prop.card))
        .chain(CARDS.cards().iter().copied())
        .find(|partner| {
            let face = partner.front();
            def.name != partner.name
                && !face.types.contains(Types::LAND)
                && (zone != Zone::Battlefield
                    || !face.types.intersects(Types::INSTANT.union(Types::SORCERY)))
                && face.cost.mana_value() <= 5
                && matches(filter, partner, player, zone)
        })
        .map(|partner| partner.name)
}

fn source_name(def: &CardDef, place: Place) -> &'static str {
    match place {
        Place::Battlefield(face) => def.face(face).name,
        _ => def.name,
    }
}

/// Cross a single chapter threshold using the turn-based action at precombat main.
fn chapter_opening(def: &'static CardDef, place: Place, chapter: u8) -> Option<Opening> {
    let Place::Battlefield(face) = place else {
        return None;
    };
    let mut out = Opening::new(def, place, format!("reach chapter {chapter}"));
    out.start = out.start.at(TURN, P0, TurnStep::Draw);
    let setup = &mut out.start.setup.players[P0.index()];
    let mut permanents = setup.permanents.to_vec();
    let saga = permanents.iter_mut().find(|p| p.card == def.name)?;
    *saga = saga.counters(CounterKind::Lore, chapter - 1);
    setup.permanents = Vec::leak(permanents);
    out.stimuli = vec![Stimulus::PassUntil(TURN, TurnStep::PrecombatMain)];
    for ability in def.face(face).abilities {
        if ability
            .chapter_numbers()
            .is_some_and(|(first, last)| (first..=last).contains(&chapter))
        {
            for target in ability.targets() {
                if target.kind == TargetKind::GraveyardCard {
                    out.graveyard_target(def, target.filter);
                }
            }
        }
    }
    Some(out)
}

#[allow(clippy::too_many_lines)]
pub(super) fn open_recipe(def: &'static CardDef, place: Place, recipe: Recipe) -> Option<Opening> {
    let new = |label: &str| Opening::new(def, place, label);
    let source = source_name(def, place);
    let out = match recipe {
        // Existing openings already play the source from each zone where the engine offers it.
        Recipe::PlaySource => return None,
        Recipe::Chapter(chapter) => return chapter_opening(def, place, chapter),
        Recipe::Attack(_) | Recipe::AttackWithout(_) => {
            let player = match recipe {
                Recipe::Attack(player) => player,
                _ => P0,
            };
            let mut out = new(if player == P0 {
                "attack with your creatures"
            } else {
                "face an opponent attack"
            });
            if player == P0 && !def.front().types.contains(Types::CREATURE) {
                out.permanent(P0, props::VANILLA.card);
            }
            if player == P1 {
                // One untapped ground attacker can be blocked by a ground creature.
                let setup = &mut out.start.setup.players[P1.index()];
                setup.permanents = Vec::leak(
                    setup
                        .permanents
                        .iter()
                        .copied()
                        .filter(|p| p.card != props::FLIER.card)
                        .collect(),
                );
            }
            if let Recipe::AttackWithout(filter) = recipe {
                let setup = &mut out.start.setup.players[P1.index()];
                setup.permanents = Vec::leak(
                    setup
                        .permanents
                        .iter()
                        .copied()
                        .filter(|p| {
                            CARDS
                                .get(p.card)
                                .is_none_or(|def| !matches(filter, def, P1, Zone::Battlefield))
                        })
                        .collect(),
                );
                out.permanent(P0, props::FLIER.card);
            }
            out.stimuli.push(Stimulus::PassUntil(
                if player == P0 { TURN } else { LAST_TURN },
                TurnStep::EndOfCombat,
            ));
            out.stimuli.push(Stimulus::Keep);
            out
        }
        Recipe::Step(player, step) => {
            let mut out = new("wait for a step");
            out.stimuli.push(Stimulus::PassUntil(
                if player == P0 { TURN } else { LAST_TURN },
                step,
            ));
            out
        }
        Recipe::BeginTurnWithTappedSource => {
            let mut out = new("begin your turn with the source tapped");
            out.start = out.start.at(TURN - 1, P1, TurnStep::End);
            let setup = &mut out.start.setup.players[P0.index()];
            let mut permanents = setup.permanents.to_vec();
            let permanent = permanents.iter_mut().find(|p| p.card == def.name)?;
            *permanent = permanent.tapped();
            setup.permanents = Vec::leak(permanents);
            out.stimuli
                .extend([Stimulus::PassUntil(TURN, TurnStep::Upkeep), Stimulus::Keep]);
            out
        }
        Recipe::Cast(player, filter, zone) => {
            let card = cast_partner(def, player, filter, zone)?;
            let mut out = new("cast a matching spell");
            out.own_turn(player);
            out.cast(def, player, card);
            let cost = CARDS.get(card)?.front().cost;
            for (land, count) in [
                names::PLAINS,
                names::ISLAND,
                names::SWAMP,
                names::MOUNTAIN,
                names::FOREST,
                names::WASTES,
            ]
            .into_iter()
            .zip(cost.symbols)
            {
                for _ in 0..count {
                    out.permanent(player, land);
                }
            }
            if player == P1 {
                out.permanent(player, names::ISLAND);
                out.permanent(player, names::ISLAND);
            }
            out
        }
        Recipe::PlayLand(player, filter) => {
            let land = [
                names::PLAINS,
                names::ISLAND,
                names::SWAMP,
                names::MOUNTAIN,
                names::FOREST,
            ]
            .into_iter()
            .filter(|&land| land != def.name)
            .find(|land| {
                CARDS
                    .get(land)
                    .is_some_and(|land| matches(filter, land, player, Zone::Battlefield))
            })?;
            let mut out = new("play a land");
            out.own_turn(player);
            out.hand(player, land);
            out.stimuli
                .push(Stimulus::Try(Action::PlayLand { player, card: land }));
            out
        }
        Recipe::Draw(player) | Recipe::DrawFromShortLibrary(player) => {
            let mut out = new("draw outside the draw step");
            out.own_turn(player);
            if matches!(recipe, Recipe::DrawFromShortLibrary(_)) {
                out.start.setup.players[player.index()].library = &[names::ISLAND];
                out.cast(def, player, names::THIRST_FOR_DISCOVERY);
            } else {
                out.cast(def, player, names::TWISTED_IMAGE);
                out.aim(player, props::FLIER.card);
            }
            out
        }
        Recipe::Discard(player, filter) => {
            let mut out = new("draw and discard");
            let partner = names::JACE_VRYNS_PRODIGY;
            if partner == def.name {
                return None;
            }
            // Looting once gives the discard trigger one object to act on, even
            // when the player has several cards they could discard.
            out.permanent(player, partner);
            out.admit(def, partner);
            if let Some(card) = cast_partner(def, player, filter, Zone::Hand) {
                out.hand(player, card);
                out.admit(def, card);
            }
            out.own_turn(player);
            out.stimuli
                .push(Stimulus::Try(Action::activate(player, partner, 0)));
            out
        }
        Recipe::GainLife(recipient) => {
            let player = if recipient == P0 { P1 } else { P0 };
            let mut out = new("gain life");
            if recipient == P0 && !def.front().types.contains(Types::CREATURE) {
                out.permanent(P0, props::VANILLA.card);
            }
            out.own_turn(player);
            out.cast(def, player, props::SORCERY.card);
            out
        }
        Recipe::LoseLife(player, threshold) => {
            let mut out = new("lose life by paying for a land");
            out.start.setup.players[player.index()].life = i16::from(threshold) + 1;
            out.own_turn(player);
            out.hand(player, props::REMOVAL.card);
            out.stimuli.push(Stimulus::Try(Action::PlayLand {
                player,
                card: props::REMOVAL.card,
            }));
            out
        }
        Recipe::Escape => {
            let mut out = new("cast a spell with granted escape");
            let card = props::SORCERY.card;
            out.start.setup.players[P0.index()].hand = &[names::PLAINS];
            out.start.setup.players[P0.index()].graveyard =
                Vec::leak(vec![card, names::PLAINS, names::ISLAND, names::MOUNTAIN]);
            out.stimuli.push(Stimulus::Try(Action::Cast {
                player: P0,
                card,
                face: FaceIx::FRONT,
                costs: Castings::ESCAPE,
            }));
            out
        }
        Recipe::PermitFromGraveyard(ability) => {
            let mut out = new("use a casting permission from the graveyard");
            let spell = props::SORCERY.card;
            let setup = &mut out.start.setup.players[P0.index()];
            setup.graveyard = Vec::leak(vec![spell]);
            setup.hand = Vec::leak(
                setup
                    .hand
                    .iter()
                    .copied()
                    .filter(|&card| card != spell)
                    .collect(),
            );
            if let Some(index) = ability {
                out.stimuli
                    .push(Stimulus::Try(Action::activate(P0, source, index)));
            } else {
                out.cast(def, P0, def.name);
            }
            out.stimuli
                .push(Stimulus::CastFrom(P0, spell, Zone::Graveyard));
            out
        }
        Recipe::PermitFromExile => {
            let mut out = new("use a casting permission from exile");
            let spell = props::VANILLA.card;
            out.start.setup.players[P1.index()].hand = Vec::leak(vec![spell, names::PLAINS]);
            out.permanent(P1, names::ISLAND);
            out.permanent(P1, names::ISLAND);
            out.cast(def, P0, def.name);
            out.own_turn(P1);
            out.stimuli.push(Stimulus::CastFrom(P1, spell, Zone::Exile));
            out
        }
        Recipe::ExileCreature => {
            let mut out = new("exile a creature");
            out.cast(def, P0, props::ARTIFACT.card);
            out.aim(P0, props::VANILLA.card);
            out
        }
        Recipe::DamageCreature | Recipe::DamagePlayer => {
            let mut out = new(if recipe == Recipe::DamageCreature {
                "deal damage to a creature"
            } else {
                "deal damage to a player"
            });
            out.cast(def, P0, names::LIGHTNING_BOLT);
            if recipe == Recipe::DamageCreature {
                out.aim(P0, props::FLIER.card);
            } else {
                out.stimuli.push(Stimulus::Aim(Action::answer(
                    P0,
                    DecisionKind::Target,
                    beryl_scenario::Pick::Player { player: P1 },
                )));
            }
            out
        }
        Recipe::Destroy(player) | Recipe::CreateTokens(player) => {
            let creates = matches!(recipe, Recipe::CreateTokens(_));
            let caster = if creates {
                if player == P0 { P1 } else { P0 }
            } else {
                player
            };
            let mut out = new(if creates {
                "create tokens"
            } else {
                "destroy a creature"
            });
            let mut target = props::FLIER.card;
            if caster == P1 {
                out.permanent(P1, names::PLAINS);
                if !def.front().types.contains(Types::CREATURE) || creates {
                    out.permanent(P0, props::VANILLA.card);
                    target = props::VANILLA.card;
                } else {
                    target = source;
                }
            }
            out.own_turn(caster);
            out.cast(def, caster, names::GET_LOST);
            out.aim(caster, target);
            out
        }
        Recipe::RemoveSource | Recipe::TargetSource => {
            let mut out = new("target the source with removal");
            if place == Place::Hand {
                out.permanent(P0, props::VANILLA.card);
                let setup = &mut out.start.setup.players[P1.index()];
                setup.graveyard = Vec::leak([setup.graveyard, &[props::VANILLA.card]].concat());
                out.cast(def, P0, def.name);
            }
            out.own_turn(P1);
            out.cast(def, P1, props::REMOVAL.card);
            out.aim(P1, source);
            out
        }
        Recipe::ReturnCreature => {
            let mut out = new("return a creature from exile");
            out.cast(def, P0, names::FLICKERWISP);
            out.aim(P0, props::FLIER.card);
            out.stimuli.push(Stimulus::PassUntil(TURN, TurnStep::End));
            out
        }
        Recipe::Mutate => {
            let mut out = new("mutate a creature");
            out.permanent(P0, props::VANILLA.card);
            let (index, _) = def
                .front()
                .casting
                .iter()
                .enumerate()
                .find(|(_, option)| matches!(option, CastingOption::Mutate { .. }))?;
            out.stimuli.push(Stimulus::Try(Action::Cast {
                player: P0,
                card: def.name,
                face: FaceIx::FRONT,
                costs: Castings::only(u8::try_from(index).ok()?),
            }));
            out
        }
        Recipe::RemoveLastCounter(kind) => {
            let mut out = new("remove the source's last counter");
            let (index, _) = def.front().abilities.iter().enumerate().find(|(_, a)| {
                matches!(a.body(), Behavior::Data(Effect::RemoveCounters { kind: removed, .. }) if *removed == kind)
            })?;
            let setup = &mut out.start.setup.players[P0.index()];
            let mut permanents = setup.permanents.to_vec();
            let permanent = permanents.iter_mut().find(|p| p.card == def.name)?;
            *permanent = permanent.counters(kind, 1);
            setup.permanents = Vec::leak(permanents);
            out.stimuli.push(Stimulus::Try(Action::activate(
                P0,
                source,
                u8::try_from(index).ok()?,
            )));
            out
        }
        Recipe::EmptyHand => {
            let mut out = new("observe the source with an empty hand");
            out.start.setup.players[P0.index()].hand = &[];
            out.stimuli
                .push(Stimulus::PassUntil(TURN, TurnStep::PostcombatMain));
            out.stimuli.push(Stimulus::Keep);
            out
        }
        Recipe::ObserveState | Recipe::WithoutPermanent(_) => {
            let mut out = new("wait for the state condition");
            if let Recipe::WithoutPermanent(filter) = recipe {
                for player in Seat::BOTH {
                    let setup = &mut out.start.setup.players[player.index()];
                    setup.permanents = Vec::leak(
                        setup
                            .permanents
                            .iter()
                            .copied()
                            .filter(|p| {
                                p.card == def.name
                                    || CARDS.get(p.card).is_none_or(|def| {
                                        !matches(filter, def, player, Zone::Battlefield)
                                    })
                            })
                            .collect(),
                    );
                }
            }
            out.stimuli
                .push(Stimulus::PassUntil(TURN, TurnStep::PostcombatMain));
            out
        }
    };
    Some(out)
}
