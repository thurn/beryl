//! Building a [`PlayerView`] from one player's [`Observation`].

use beryl_engine::card::{
    Ability, AbilityKind, CastingOption, FaceAbility, FaceIx, Measure, Part, Purpose, Shape, Types,
};
use beryl_engine::event::{Event, LossCause};
use beryl_engine::layout::Queue;
use beryl_engine::state::{ObjectFlags, ObjectRef, Pending, StackEntry};
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{
    Candidates, Decision, GameTable, Layout, Observation, Opt, Outcome, Seat, SlotId, Small, State,
    Zone, ZoneKey,
};

use super::{
    CardHandling, CardType, Combat, CounterView, DecisionId, Facing, Keyword, ManaPool, ObjectView,
    PaidCost, PlayerState, PlayerView, Players, PowerToughness, Side, StackItem, Status,
    StopsSetting, TargetRef, TurnView, decision_view, game_events, halt_view, outcome_view,
};

/// Where the game stands, as the session knows it: what [`player_view`] turns into a [`Status`].
#[derive(Clone, Copy, Debug)]
pub enum Moment<'a, L: Layout = Small> {
    /// The viewer's decision `id` is open, asked by the resolution of the stack entry given,
    /// if one asks it.
    Deciding(DecisionId, &'a Decision<L>, Option<&'a StackEntry<L>>),
    /// The viewer is halted ([`DecisionView::halt`](super::DecisionView::halt)) with decision `id`.
    Halted(DecisionId),
    /// The other player is deciding, in the resolution of the stack entry given if one asks
    /// it.
    Thinking {
        resolving: Option<&'a StackEntry<L>>,
        decision: Option<&'a Decision<L>>,
    },
    Playing,
    Stopped,
    /// The game is over; `cause`, why the loser lost, when the session saw it
    /// ([`Event::Lose`]), and `hurt_by`, the source of the damage that last lowered their
    /// life, if damage rather than a loss of life did.
    Over {
        outcome: Outcome,
        cause: Option<LossCause<L>>,
        hurt_by: Option<ObjectRef<L>>,
    },
}

/// Builds the viewer's view of `obs`'s position (the position the open decision is made in,
/// `beryl_engine::view`) at `moment`, with `events`, the engine's events since the previous view.
/// Everything in it, events included, comes through `obs`.
pub fn player_view<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    moment: Moment<'_, L>,
    stops: StopsSetting,
    events: &[Event<L>],
) -> PlayerView {
    let turn = obs.turn();
    let mut objects = Vec::new();
    let mut player_state = |p: Seat| {
        let state = player_state(t, obs, p);
        for slots in [&state.library, &state.graveyard] {
            objects.extend(slots.iter().map(|&slot| {
                object_view(t, obs, L::Slot::from_num(slot).expect("a slot of the game"))
            }));
        }
        // Unordered zones: their objects say where they are.
        for slots in [Zone::Hand, Zone::Exile].map(|zone| obs.zone(ZoneKey::Of(p, zone))) {
            let slots = obs.in_candidate_order(slots);
            objects.extend(slots.map(|slot| object_view(t, obs, slot)));
        }
        state
    };
    let viewer = obs.viewer();
    let players = Players {
        you: player_state(viewer),
        opponent: player_state(viewer.opponent()),
    };
    objects.extend(obs.battlefield().iter().map(|&s| object_view(t, obs, s)));
    let mut stack: Vec<_> = obs.stack().iter().map(|e| stack_item(t, obs, e)).collect();
    objects.extend(
        obs.stack()
            .iter()
            .filter_map(StackEntry::spell)
            .map(|r| object_view(t, obs, r.slot)),
    );
    // A resolving spell's card stays on the stack until its resolution ends
    // ([CR 608.2n](https://yawgatog.com/resources/magic-rules/#R6082n): "As the final part of
    // an instant or sorcery spell's resolution, the spell is put into its owner's
    // graveyard").
    let resolving = match moment {
        Moment::Deciding(_, _, entry)
        | Moment::Thinking {
            resolving: entry, ..
        } => entry,
        _ => None,
    };
    if let Some(entry) = resolving
        && let Some(r) = entry.spell()
        && obs.object(r.slot).zone() == Zone::Stack
    {
        objects.push(object_view(t, obs, r.slot));
    }
    // While the other player answers a question of its resolution, the entry stays on top of
    // the viewer's stack: the viewer's own questions name it apart ([`DecisionView::resolving`]).
    if let Moment::Thinking {
        resolving: Some(entry),
        ..
    } = moment
    {
        stack.push(stack_item(t, obs, entry));
    }
    objects.sort_by_key(|o| o.slot);
    let status = match moment {
        Moment::Deciding(id, d, resolving) => {
            debug_assert_eq!(d.player, viewer, "a view shows only its viewer's decision");
            Status::Deciding {
                decision: decision_view(t, obs, id, d, resolving, stops),
            }
        }
        Moment::Halted(id) => Status::Deciding {
            decision: halt_view(t, obs, id, stops),
        },
        Moment::Thinking { .. } => Status::Thinking,
        Moment::Playing => Status::Playing,
        Moment::Stopped => Status::Stopped,
        Moment::Over { .. } => Status::Over {
            outcome: outcome_view(t, obs, moment),
        },
    };
    let handling = match moment {
        Moment::Thinking {
            decision: Some(d), ..
        } if d.player != viewer => card_handling(obs, d),
        _ => None,
    };
    PlayerView {
        status,
        handling,
        pending_attackers: pending_attackers(obs, moment),
        turn: TurnView {
            number: turn.number(),
            active: side(obs, turn.active()),
            step: turn.step().into(),
            priority: side(obs, turn.priority()),
            starting: obs.starting_player().map(|p| side(obs, p)),
            flip_winner: None,
        },
        players,
        battlefield: slots(obs.battlefield().iter().copied()),
        objects,
        stack,
        stops,
        events: game_events(t, obs, events),
        undone: false,
        script: None,
    }
}

pub(super) fn pending_attackers<L: Layout>(
    obs: Observation<'_, L>,
    moment: Moment<'_, L>,
) -> Option<Vec<u16>> {
    let decision = match moment {
        Moment::Deciding(_, d, _)
        | Moment::Thinking {
            decision: Some(d), ..
        } => Some(d),
        _ => None,
    };
    // The aim of an attack names the creatures being aimed.
    decision
        .filter(|d| d.prompt.purpose == Purpose::Attack && d.prompt.part == Part::Aim)
        .map(|d| slots(obs.in_candidate_order(d.about)))
}

fn player_state<L: Layout>(t: &GameTable, obs: Observation<'_, L>, p: Seat) -> PlayerState {
    let player = obs.player(p);
    let [white, blue, black, red, green, colorless] = player.mana().amounts();
    PlayerState {
        life: player.life(),
        mana: ManaPool {
            white,
            blue,
            black,
            red,
            green,
            colorless,
        },
        lands_played: player.lands_played(),
        mulligans: player.mulligans(),
        mulligan: player.mulligan().into(),
        library: slots(obs.library(p).iter().copied()),
        graveyard: slots(obs.graveyard(p).iter().copied()),
        emblems: player
            .emblems()
            .iter()
            .map(|&e| t.def(e).name.to_owned())
            .collect(),
    }
}

/// The first of the cards `d`'s prompt shows: a card of its object set, or else an object it
/// offers. The cards shown share an owner and a zone, so the first in candidate order says
/// whose they are and where they lie.
pub(crate) fn shown<L: Layout>(obs: Observation<'_, L>, d: &Decision<L>) -> Option<L::Slot> {
    let about = obs.in_candidate_order(d.about).next();
    about.or_else(|| match &d.candidates {
        Candidates::Objects(_) => d.candidates.objects().next(),
        Candidates::Options(opts) => opts.iter().find_map(|&o| match o {
            Opt::Object(slot) => Some(slot),
            _ => None,
        }),
    })
}

/// Takes off the stack of `view`, a view of the game `s`, an opponent's ability whose cost is
/// still being paid by tapping permanents in place of mana: the viewer watches the permanents
/// tap before the activation lifts from its source. The controller's own view keeps the entry,
/// which says what they are paying for. A game does this as it draws a view; a view built from
/// a position alone ([`player_view`]) has it done here.
pub fn hold_back_activation_being_paid<L: Layout>(view: &mut PlayerView, s: &State<L>) {
    if matches!(s.pending(), Pending::Casting(_))
        && matches!(
            view.stack.last(),
            Some(StackItem::Ability {
                controller: Side::Opponent,
                taps_to_pay: Some(true),
                ..
            })
        )
    {
        view.stack.pop();
    }
}

/// What the opponent sees of the cards `d`'s prompt has its player look through: the hand or
/// the library its object set lies in, or else its candidates, and how much of it.
fn card_handling<L: Layout>(obs: Observation<'_, L>, d: &Decision<L>) -> Option<CardHandling> {
    // A yes or no, or a mode, about a card shows the opponent nothing more than the table
    // does: the card is revealed (an explore's) or being played (a land that may cost life, a
    // card offered to cast). The two that are looks at hidden cards do.
    if matches!(d.prompt.shape(), Shape::YesNo | Shape::Mode)
        && !matches!(d.prompt.purpose, Purpose::Fateseal | Purpose::Mulligan)
    {
        return None;
    }
    let card = obs.object(shown(obs, d)?);
    let player = card.owner();
    match card.zone() {
        Zone::Library => {
            // The top cards the prompt counts, or the whole library.
            let size = obs.library(player).len();
            let count = (d.prompt.count > 0)
                .then(|| d.prompt.count.min(u8::try_from(size).unwrap_or(u8::MAX)));
            Some(CardHandling::Library {
                side: side(obs, player),
                count,
                scry: (d.prompt.purpose == Purpose::Scry).then_some(true),
            })
        }
        Zone::Hand => Some(CardHandling::Hand {
            side: side(obs, player),
        }),
        _ => None,
    }
}

/// Player `p` as the viewer of `obs` sees them.
pub(crate) fn side<L: Layout>(obs: Observation<'_, L>, p: Seat) -> Side {
    if p == obs.viewer() {
        Side::You
    } else {
        Side::Opponent
    }
}

/// Slots as the protocol numbers them. A set of objects is sent in the candidate order.
pub(crate) fn slots<S: SlotId>(slots: impl IntoIterator<Item = S>) -> Vec<u16> {
    slots.into_iter().map(SlotId::num).collect()
}

/// The name of the face up of the object in `slot`, if the viewer knows it; for a face-down
/// permanent, the name of its card, which it turns face up with (CR 712.12a).
pub(crate) fn name<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    slot: L::Slot,
) -> Option<&'static str> {
    let face = match obs.object(slot).face() {
        FaceIx::DOWN => FaceIx::FRONT,
        face => face,
    };
    face_name(t, obs, slot, face)
}

/// The name of face `face` of the object in `slot`, if the viewer knows it: a copy's is the
/// copied card's (CR 707.2), since a token copy is a token of that card.
pub(super) fn face_name<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    slot: L::Slot,
    face: FaceIx,
) -> Option<&'static str> {
    Some(t.def(obs.card(slot)?).face_name(face))
}

fn object_view<L: Layout>(t: &GameTable, obs: Observation<'_, L>, slot: L::Slot) -> ObjectView {
    let o = obs.object(slot);
    let flags = o.flags();
    let on_battlefield = o.zone() == Zone::Battlefield;
    let derived = on_battlefield.then(|| obs.derived(slot));
    let types = derived.map_or(Types::NONE, |d| d.types);
    let creature = types.contains(Types::CREATURE);
    let counters = obs
        .counters(slot)
        .iter()
        .map(|(kind, count)| CounterView {
            kind: kind.into(),
            count,
        })
        .collect();
    let some = |s: Option<L::Slot>| s.map(SlotId::num);
    ObjectView {
        slot: slot.num(),
        zone: o.zone().into(),
        owner: side(obs, o.owner()),
        controller: side(obs, obs.controller(slot)),
        name: name(t, obs, slot).map(str::to_owned),
        facing: if o.face() == FaceIx::DOWN {
            Facing::Down
        } else {
            Facing::Up
        },
        token: flags.has(ObjectFlags::TOKEN),
        tapped: flags.has(ObjectFlags::TAPPED),
        summoning_sick: creature && flags.has(ObjectFlags::SUMMONING_SICK),
        damage: o.damage(),
        counters,
        types: CardType::ALL
            .iter()
            .filter(|&&(bit, _)| types.contains(bit))
            .map(|&(_, ty)| ty)
            .collect(),
        pt: derived.filter(|_| creature).map(|d| PowerToughness {
            power: d.power,
            toughness: d.toughness,
        }),
        keywords: derived.map_or_else(Vec::new, |d| {
            Keyword::ALL
                .iter()
                .filter(|&&(bit, _)| d.keywords.contains(bit))
                .map(|&(_, k)| k)
                .collect()
        }),
        attached_to: some(o.attached_to()),
        exiled_by: some(obs.memos(slot).exiled_until()),
        exiled_with: some(obs.memos(slot).exiled_with()),
        combat: if flags.has(ObjectFlags::ATTACKING) {
            let attacks_walker = flags.has(ObjectFlags::ATTACKS_PLANESWALKER);
            let planeswalker = if attacks_walker {
                some(o.combat())
            } else {
                None
            };
            Some(Combat::Attacking {
                planeswalker,
                target_departed: (attacks_walker && planeswalker.is_none()).then_some(true),
                blocked: flags.has(ObjectFlags::BLOCKED),
            })
        } else if flags.has(ObjectFlags::BLOCKING) {
            Some(Combat::Blocking {
                attacker: some(o.combat()),
            })
        } else {
            None
        },
    }
}

/// Whether ability `index` of `list` is triggered, and how many paragraphs of the face's text
/// say abilities of its kind (triggered, or activated) before the one that says it, so the UI
/// finds its paragraph.
pub(super) fn ability_order(list: &[Ability], index: u8) -> (bool, u8) {
    // Triggered, or activated (mana abilities are written as activated ones are); the other
    // kinds have no paragraph of their own to find.
    let class = |a: &Ability| match a.kind() {
        AbilityKind::Triggered => Some(true),
        AbilityKind::Activated | AbilityKind::Mana => Some(false),
        _ => None,
    };
    let kind = list.get(usize::from(index)).and_then(class) == Some(true);
    let nth = paragraphs_before(list, usize::from(index), |a| class(a) == Some(kind));
    (kind, nth)
}

/// How many paragraphs of a face's text say abilities of one kind (those `of_kind` accepts)
/// before the one that says ability `index` of `list`: an ability said in the paragraph of
/// the one before it ([`Ability::shares_paragraph`]) has none of its own, as the attack
/// trigger of "Whenever this creature enters or attacks" does.
pub(super) fn paragraphs_before(
    list: &[Ability],
    index: usize,
    of_kind: impl Fn(&Ability) -> bool,
) -> u8 {
    let own = |a: &&Ability| of_kind(a) && !a.shares_paragraph();
    let before = list.iter().take(index).filter(own).count();
    // Said in the paragraph before its own, it is the last of those counted.
    let shares = list
        .get(index)
        .is_some_and(|a| of_kind(a) && a.shares_paragraph());
    u8::try_from(before - usize::from(shares)).expect("a face has fewer than 256 abilities")
}

/// The costs the spell `e` was cast with: a flashback cost, which is an alternative cost
/// ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a)), or else the options of
/// its face that were paid.
fn paid_costs<L: Layout>(t: &GameTable, e: &StackEntry<L>, flashback: bool) -> Vec<PaidCost> {
    if flashback {
        return vec![PaidCost::Flashback];
    }
    e.costs
        .paid(t.def(e.card).face(e.face))
        .map(|option| match option {
            CastingOption::Kicker { .. } => PaidCost::Kicker,
            CastingOption::Offspring(_) => PaidCost::Offspring,
            CastingOption::Impending { .. } => PaidCost::Impending,
            CastingOption::Overload { .. } => PaidCost::Overload,
            CastingOption::Evoke { .. } => PaidCost::Evoke,
        })
        .collect()
}

/// A spell or ability on the stack as the viewer of `obs` sees it. Whether a flashback
/// cost was paid for a spell is the game's to say while the spell is on the stack.
pub(super) fn stack_item<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    e: &StackEntry<L>,
) -> StackItem {
    let flashback = e
        .spell()
        .is_some_and(|spell| beryl_engine::flashback_paid(obs.shown(), spell));
    stack_item_cast(t, obs, e, flashback)
}

/// [`stack_item`] for a spell known to have been cast for a flashback cost or not, as its
/// cast said.
pub(super) fn stack_item_cast<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    e: &StackEntry<L>,
    flashback: bool,
) -> StackItem {
    let targets = e
        .targets
        .iter()
        .map(|target| match (target.as_player(), target.as_object()) {
            (Some(player), _) => TargetRef::Player {
                player: side(obs, player),
            },
            (None, Some(o)) => TargetRef::Object { slot: o.slot.num() },
            (None, None) => unreachable!("a target is a player or an object"),
        })
        .collect();
    let source = e.object.map(|r| r.slot.num());
    match e.ability {
        None => StackItem::Spell {
            slot: source.expect("a spell's card"),
            targets,
            x: e.x,
            paid: paid_costs(t, e, flashback),
        },
        Some(ability) => {
            let face = t.def(e.card).face(e.face);
            let (list, index) = match ability {
                FaceAbility::Own(i) => (face.abilities, i.get()),
                FaceAbility::Grant(i) => (face.grants, i.get()),
            };
            let (kind, nth) = ability_order(list, index);
            StackItem::Ability {
                source,
                ability: index,
                granted: matches!(ability, FaceAbility::Grant(_)),
                name: t.def(e.card).face_name(e.face).to_owned(),
                triggered: kind,
                taps_to_pay: matches!(
                    face.ability(ability).cost().measure(),
                    Some(Measure::ForGeneric(_))
                )
                .then_some(true),
                nth,
                controller: side(obs, e.controller),
                targets,
                x: e.x,
            }
        }
    }
}
