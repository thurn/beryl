//! Building a [`PlayerView`] from one player's [`Observation`].

use beryl_engine::card::{
    Ability, AbilityKind, CastingOption, Designation, FaceAbility, FaceIx, Keywords, Measure, Part,
    Purpose, Types,
};
use beryl_engine::event::{Event, LossCause};
use beryl_engine::layout::Queue;
use beryl_engine::state::{
    AbilityTarget, AttackTarget, CombatRole, Memo, ObjectRef, Pending, StackEntry,
};
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{
    Decision, GameTable, Layout, Observation, Outcome, Seat, SlotId, Small, State, Zone, ZoneKey,
};

use super::{
    CardType, Combat, CounterView, DecisionId, DesignationView, Facing, Keyword, ManaPool,
    ObjectId, ObjectView, PaidCost, PlayerState, PlayerView, Players, PowerToughness,
    SHARED_PILE_SIDE, SharedPiles, Side, StackItem, Status, StopsSetting, TargetRef, TurnView,
    decision_view, game_events, halt_view, outcome_view, own_number,
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

/// The status of the view: the viewer's decision, or what the game is doing instead.
fn status_view<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    moment: Moment<'_, L>,
    stops: StopsSetting,
) -> Status {
    match moment {
        Moment::Deciding(id, d, resolving) => {
            debug_assert_eq!(
                d.decider,
                obs.viewer(),
                "a view shows only its viewer's decision"
            );
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
    }
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
    let piles = |objects: &mut Vec<ObjectView>, library: &[ObjectId], graveyard: &[ObjectId]| {
        for slots in [library, graveyard] {
            objects.extend(slots.iter().map(|&slot| {
                object_view(
                    t,
                    obs,
                    L::Slot::from_num(slot.0).expect("a slot of the game"),
                )
            }));
        }
    };
    // A shared library and graveyard are sent once, as no player's
    // ([`PlayerView::shared`]).
    let shared = obs.shown().shares_zones().then(|| SharedPiles {
        library: slots(obs.library(obs.viewer()).iter().copied()),
        graveyard: slots(obs.graveyard(obs.viewer()).iter().copied()),
    });
    if let Some(shared) = &shared {
        piles(&mut objects, &shared.library, &shared.graveyard);
    }
    let mut player_state = |p: Seat| {
        let state = player_state(t, obs, p);
        piles(&mut objects, &state.library, &state.graveyard);
        // Unordered zones: their objects say where they are.
        for slots in [Zone::Hand, Zone::Exile].map(|zone| obs.zone(ZoneKey::Of(p, zone))) {
            let slots = slots_by_id(slots);
            objects.extend(slots.map(|slot| object_view(t, obs, slot)));
        }
        state
    };
    let viewer = obs.viewer();
    let players = Players {
        you: player_state(viewer),
        opponent: player_state(viewer.opponent()),
    };
    let phased_out = add_battlefield_objects(t, obs, &mut objects);
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
    finish_objects(t, obs, &mut objects);
    let status = status_view(t, obs, moment, stops);
    let handling = match moment {
        Moment::Thinking {
            decision: Some(d), ..
        } if d.decider != viewer => super::handling::card_handling(obs, d),
        _ => None,
    };
    PlayerView {
        status,
        handling,
        shared,
        pending_attackers: pending_attackers(moment),
        turn: TurnView {
            number: turn.number(),
            own_number: own_number(turn.number()),
            active: side(obs, turn.active()),
            step: turn.step().into(),
            priority: Some(side(obs, turn.priority())),
            starting: obs.starting_player().map(|p| side(obs, p)),
            flip_winner: None,
            controlled_by: turn.controller().map(|p| side(obs, p)),
        },
        players,
        battlefield: slots(obs.battlefield().iter().chain(&phased_out).copied()),
        objects,
        stack,
        stops,
        events: game_events(t, obs, events),
        undone: false,
        script: None,
        table: None,
    }
}

/// Puts `objects` in slot order and marks the viewer's own graveyard cards among them that
/// they may cast from there, which the table shows in their hand. Whether a card may be cast
/// that way is public: the graveyard is, and so is the effect that allowed it.
fn finish_objects<L: Layout>(t: &GameTable, obs: Observation<'_, L>, objects: &mut [ObjectView]) {
    objects.sort_by_key(|o| o.slot);
    let castable = beryl_engine::castable_from_graveyard(obs.shown(), t, obs.viewer());
    for object in objects.iter_mut() {
        object.castable_from_graveyard = object.zone == super::ZoneName::Graveyard
            && L::Slot::from_num(object.slot.0).is_some_and(|slot| castable.contains(slot));
    }
}

fn add_battlefield_objects<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    objects: &mut Vec<ObjectView>,
) -> Vec<L::Slot> {
    objects.extend(obs.battlefield().iter().map(|&s| object_view(t, obs, s)));
    // The other cards of each merged permanent, which the UI draws under it ([CR 731.2b]: "As
    // an object merges with a permanent, that object leaves its previous zone and becomes part
    // of an object on the battlefield").
    //
    // [CR 731.2b]: https://yawgatog.com/resources/magic-rules/#R7312b
    // A merged permanent with a mutating card over it keeps its own card in its slot, under a
    // copy effect: the card in the merged zone that it shows is the one over it, so the view
    // names the card under it by the permanent's own.
    // A card under it that was face down when the mutating card went over it stays face down,
    // and the view of a player who never saw it draws it so, with no name ([CR 731.2e]): "If
    // a merged permanent contains face-up and face-down components, the permanent's status is
    // determined by its topmost component."
    //
    // [CR 731.2e]: https://yawgatog.com/resources/magic-rules/#R7312e
    for &host in obs.battlefield() {
        let mut own = really(t, obs, host);
        let mut hidden = obs.shown().really(host).is_some() && obs.really(host).is_none();
        let top = name(t, obs, host);
        for s in obs.shown().pile(host) {
            let mut view = object_view(t, obs, s);
            if own.is_some() && view.name.as_deref() == top {
                view.name = own.take().map(str::to_owned);
                view.other_face = obs
                    .really(host)
                    .and_then(|card| t.def(card).other_face_name(FaceIx::FRONT))
                    .map(str::to_owned);
            } else if hidden && view.name.as_deref() == top {
                hidden = false;
                view.name = None;
                view.other_face = None;
                view.facing = Facing::Down;
            }
            objects.push(view);
        }
    }
    // Phased-out permanents are on the battlefield by their zone, but in no battlefield list:
    // the view still sends them, marked, after the rest ([CR 702.26b]): "a phased-out
    // permanent is treated as though it does not exist".
    //
    // [CR 702.26b]: https://yawgatog.com/resources/magic-rules/#R70226b
    let phased_out = slots_by_id(obs.phased_out());
    let phased_out: Vec<L::Slot> = phased_out.collect();
    objects.extend(phased_out.iter().map(|&s| object_view(t, obs, s)));
    phased_out
}

pub(super) fn pending_attackers<L: Layout>(moment: Moment<'_, L>) -> Option<Vec<ObjectId>> {
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
        .map(|d| slots(slots_by_id(d.about)))
}

fn player_state<L: Layout>(t: &GameTable, obs: Observation<'_, L>, p: Seat) -> PlayerState {
    let player = obs.player(p);
    // Mana that remembers what it may be spent on counts as mana of its kind (CR 106.6: "This
    // doesn't affect the mana's type").
    let mut amounts = player
        .mana()
        .amounts()
        .map(beryl_engine::UnsignedNumber::to_u32);
    for mana in player.remembered_mana() {
        amounts[mana.color.index()] += 1;
    }
    let [white, blue, black, red, green, colorless] = amounts;
    PlayerState {
        life: beryl_engine::SignedNumber::to_i32(player.life()),
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
        library: own_pile(obs, obs.library(p)),
        graveyard: own_pile(obs, obs.graveyard(p)),
        emblems: player
            .emblems()
            .iter()
            .map(|&e| t.def(e).name.to_owned())
            .collect(),
        named_counters: Vec::new(),
    }
}

/// A player's own library or graveyard, `pile`: empty in a shared-deck game, whose piles are
/// [`PlayerView::shared`]'s.
fn own_pile<L: Layout>(obs: Observation<'_, L>, pile: &[L::Slot]) -> Vec<ObjectId> {
    if obs.shown().shares_zones() {
        Vec::new()
    } else {
        slots(pile.iter().copied())
    }
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

/// Sets [`ObjectView::printing`] of each object in `view` whose card the viewer knows, to
/// `printing` of its object ID: which of its name's printings the slot shows, the same in both
/// seats' views (`crate::printings`). An object the viewer does not know shows none, so a
/// card's art says nothing about which card it is.
pub fn show_printings(
    view: &mut PlayerView,
    printing: impl Fn(ObjectId) -> Option<beryl_card_records::PrintingIx>,
) {
    for o in &mut view.objects {
        o.printing = o.name.as_ref().and_then(|_| printing(o.slot));
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

/// The members of an unordered object set, in the protocol's object ID order.
pub(crate) fn slots_by_id<S: SlotId>(
    objects: beryl_engine::Bitboard<S>,
) -> impl Iterator<Item = S> {
    S::all().filter(move |&slot| objects.contains(slot))
}

/// Slots as the protocol numbers them. A set of objects is sent in the candidate order.
pub(crate) fn slots<S: SlotId>(slots: impl IntoIterator<Item = S>) -> Vec<ObjectId> {
    slots.into_iter().map(|slot| ObjectId(slot.num())).collect()
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

/// The owner and controller of the object in `slot`, as the viewer sees them. A card in a
/// shared library or graveyard has no owner: its record names one fixed side for both.
fn owner_and_controller<L: Layout>(obs: Observation<'_, L>, slot: L::Slot) -> (Side, Side) {
    if obs.shown().has_owner(slot) {
        let o = obs.object(slot);
        (side(obs, o.owner()), side(obs, obs.controller(slot)))
    } else {
        (SHARED_PILE_SIDE, SHARED_PILE_SIDE)
    }
}

fn object_view<L: Layout>(t: &GameTable, obs: Observation<'_, L>, slot: L::Slot) -> ObjectView {
    let o = obs.object(slot);
    let on_battlefield = o.zone() == Zone::Battlefield;
    let derived = characteristics(t, obs, slot);
    let types = derived.map_or(Types::NONE, |d| d.types);
    let creature = types.contains(Types::CREATURE);
    let counters = obs
        .counters(slot)
        .iter()
        .map(|(kind, count)| CounterView {
            kind: kind.into(),
            count: i32::try_from(beryl_engine::UnsignedNumber::to_u32(count)).unwrap_or(i32::MAX),
        })
        .collect();
    let some = |s: Option<L::Slot>| s.map(|s| ObjectId(s.num()));
    let changed = obs
        .card(slot)
        .and_then(|card| super::rules_text::changed(t, card, o.face(), &obs.words_of(slot)));
    let (text, type_line) = changed.map_or((None, None), |(text, line)| (Some(text), Some(line)));
    let (owner, controller) = owner_and_controller(obs, slot);
    ObjectView {
        slot: ObjectId(slot.num()),
        zone: o.zone().into(),
        owner,
        zone_side: None,
        controller,
        name: name(t, obs, slot).map(str::to_owned),
        // Which copy of its name a card is, the engine cannot say: the game sets it
        // ([`show_printings`]).
        printing: None,
        other_face: obs.other_face_name(t, slot).map(str::to_owned),
        facing: if o.face() == FaceIx::DOWN {
            Facing::Down
        } else {
            Facing::Up
        },
        token: o.is_token(),
        tapped: o.is_tapped(),
        summoning_sick: creature && o.is_summoning_sick(),
        damage: i32::try_from(beryl_engine::UnsignedNumber::to_u32(o.damage())).unwrap_or(i32::MAX),
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
        mana_ability: on_battlefield && has_mana_ability(t, obs, slot),
        attached_to: some(o.attached_to()),
        exiled_by: some(obs.memos(slot).exiled_until()),
        exiled_with: some(obs.memos(slot).exiled_with()),
        phased_out: o.is_phased_out(),
        part_of: obs.memos(slot).iter().find_map(|memo| match memo {
            Memo::PartOf(host) => Some(ObjectId(host.num())),
            _ => None,
        }),
        // A merged permanent's cards are drawn under it instead ([`player_view`]).
        really: really(t, obs, slot)
            .filter(|_| obs.shown().pile(slot).next().is_none())
            .map(str::to_owned),
        chosen: obs.shown().chosen(slot).map(|value| value.name(t)),
        text,
        type_line,
        custom_colors: None,
        token_printing: None,
        combat: match o.combat() {
            CombatRole::Attacking {
                target, blocked, ..
            } => Some(Combat::Attacking {
                planeswalker: match target {
                    AttackTarget::Planeswalker(walker) => Some(ObjectId(walker.num())),
                    AttackTarget::Player(_) | AttackTarget::Removed => None,
                },
                target_departed: (*target == AttackTarget::Removed).then_some(true),
                blocked: *blocked,
            }),
            CombatRole::Blocking { attackers, .. } => Some(Combat::Blocking {
                attacker: some(attackers.first()),
            }),
            CombatRole::None => None,
        },
        note: None,
        pinned: None,
        face: super::FaceName::Front,
        named_counters: Vec::new(),
        doesnt_untap: false,
        hidden_pick: obs.hidden_picks().contains(slot),
        castable_from_graveyard: false,
        designations: if on_battlefield {
            designations(t, obs, slot)
        } else {
            Vec::new()
        },
    }
}

/// The designations of the permanent in `slot` that the view shows ([`DesignationView`]): a
/// Class's level, from its memo or 1 without one, solved and prepared. Designations are public.
fn designations<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    slot: L::Slot,
) -> Vec<DesignationView> {
    let mut out = Vec::new();
    let face = obs.object(slot).face();
    let class = face != FaceIx::DOWN
        && obs.card(slot).is_some_and(|card| {
            t.def(card)
                .face(face)
                .abilities
                .iter()
                .any(|a| matches!(a.designation(), Designation::LevelUp(_)))
        });
    let memos = obs.memos(slot);
    if class {
        let level = memos
            .iter()
            .find_map(|memo| match memo {
                Memo::ClassLevel(level) => Some(level),
                _ => None,
            })
            .unwrap_or(1);
        out.push(DesignationView::Level { level });
    }
    if memos.iter().any(|memo| memo == Memo::Solved) {
        out.push(DesignationView::Solved);
    }
    if memos.iter().any(|memo| matches!(memo, Memo::Prepared(_))) {
        out.push(DesignationView::Prepared);
    }
    out
}

/// The name of the card the permanent in `slot` really is, when a copy effect shows another
/// ([CR 707.2]).
///
/// [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
fn really<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    slot: L::Slot,
) -> Option<&'static str> {
    if !obs.knows(slot) || obs.object(slot).zone() != Zone::Battlefield {
        return None;
    }
    let card = t.def(obs.really(slot)?).face_name(FaceIx::FRONT);
    (Some(card) != name(t, obs, slot)).then_some(card)
}

/// Whether the face up of the object in `slot` has a mana ability ([CR 605.1a]): "An activated
/// ability is a mana ability if it meets all of the following criteria". A face-down permanent
/// has none, having no text ([CR 708.2a]): "a 2/2 face-down creature with no text, no name, no
/// subtypes, and no mana cost". False for an object the viewer does not know.
///
/// [CR 605.1a]: https://yawgatog.com/resources/magic-rules/#R6051a
/// [CR 708.2a]: https://yawgatog.com/resources/magic-rules/#R7082a
fn has_mana_ability<L: Layout>(t: &GameTable, obs: Observation<'_, L>, slot: L::Slot) -> bool {
    let face = obs.object(slot).face();
    face != FaceIx::DOWN
        && obs.card(slot).is_some_and(|card| {
            t.def(card)
                .face(face)
                .abilities
                .iter()
                .any(|a| a.kind() == AbilityKind::Mana)
        })
}

/// An object's types, power, toughness and keywords as the view shows them.
#[derive(Clone, Copy)]
struct Shown {
    types: Types,
    power: i32,
    toughness: i32,
    keywords: Keywords,
}

/// The characteristics of the permanent in `slot`: after continuous effects, or for a
/// phased-out permanent, which has no layer results, those printed on its face up, as it is
/// treated as though it does not exist ([CR 702.26b]): "a phased-out permanent is treated as
/// though it does not exist". A face-down spell uses its public face ([CR 708.2a]).
///
/// [CR 708.2a]: https://yawgatog.com/resources/magic-rules/#R7082a
///
/// [CR 702.26b]: https://yawgatog.com/resources/magic-rules/#R70226b
fn characteristics<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    slot: L::Slot,
) -> Option<Shown> {
    let object = obs.object(slot);
    let face = if object.zone() == Zone::Battlefield {
        if !object.is_phased_out() {
            let d = obs.derived(slot);
            return Some(Shown {
                types: d.types,
                power: beryl_engine::SignedNumber::to_i32(d.power),
                toughness: beryl_engine::SignedNumber::to_i32(d.toughness),
                keywords: d.keywords,
            });
        }
        t.def(obs.card(slot)?).face(object.face())
    } else if object.zone() == Zone::Stack && object.face() == FaceIx::DOWN {
        &beryl_engine::card::FACE_DOWN
    } else {
        return None;
    };
    Some(Shown {
        types: face.types,
        power: i32::from(face.power.unwrap_or(0)),
        toughness: i32::from(face.toughness.unwrap_or(0)),
        keywords: face.keywords,
    })
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
    e.costs()
        .paid(t.def(e.card()).face(e.face()))
        .filter_map(|option| match option {
            CastingOption::Kicker { .. } => Some(PaidCost::Kicker),
            CastingOption::Offspring(_) => Some(PaidCost::Offspring),
            CastingOption::Impending { .. } => Some(PaidCost::Impending),
            CastingOption::Overload { .. } => Some(PaidCost::Overload),
            CastingOption::Evoke { .. } => Some(PaidCost::Evoke),
            // No keyword names it, so Arena tags nothing: Force of Will cast for its
            // alternative cost is a plain Force of Will on the stack. An escaped spell is
            // tagged nothing either, as no tag of the views says escape, and a mutating
            // spell shows its target instead.
            CastingOption::Alternative { .. }
            | CastingOption::Escape { .. }
            | CastingOption::Mutate { .. }
            | CastingOption::Morph(_)
            | CastingOption::Fuse
            | CastingOption::Prototype { .. } => None,
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
    // An ability target whose ability has left the stack names nothing to show.
    let targets = e
        .targets
        .iter()
        .filter_map(
            |target| match (target.as_player(), target.as_object(), target.as_ability()) {
                (Some(player), _, _) => Some(TargetRef::Player {
                    player: side(obs, player),
                }),
                (None, Some(o), _) => Some(TargetRef::Object {
                    slot: ObjectId(o.slot.num()),
                }),
                (None, None, Some(AbilityTarget::At(place))) => Some(TargetRef::Ability {
                    place: u16::try_from(place).expect("a stack place fits"),
                }),
                (None, None, Some(AbilityTarget::Gone)) => None,
                (None, None, None) => unreachable!("a target is a player, an object or an ability"),
            },
        )
        .collect();
    let source = e.object().map(|r| ObjectId(r.slot.num()));
    match e.ability() {
        None => StackItem::Spell {
            slot: source.expect("a spell's card"),
            targets,
            x: e.x,
            paid: paid_costs(t, e, flashback),
        },
        Some(ability) => {
            let face = t.def(e.card()).face(e.face());
            let (list, index) = match ability {
                FaceAbility::Own(i) => (face.abilities, i.get()),
                FaceAbility::Grant(i) => (face.grants, i.get()),
            };
            let (kind, nth) = ability_order(list, index);
            StackItem::Ability {
                source,
                ability: index,
                granted: matches!(ability, FaceAbility::Grant(_)),
                name: t.def(e.card()).face_name(e.face()).to_owned(),
                triggered: kind,
                taps_to_pay: matches!(
                    face.ability(ability).cost().measure(),
                    Some(Measure::ForGeneric(_))
                )
                .then_some(true),
                nth,
                text: super::rules_text::unprinted_read(
                    t,
                    e.card(),
                    e.face(),
                    ability,
                    &obs.entry_words(e),
                ),
                controller: side(obs, e.controller()),
                manual: None,
                targets,
                x: e.x,
            }
        }
    }
}
