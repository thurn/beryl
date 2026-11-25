// Generated from the card store by `just card header`: notes go below the end marker.
//! Arcane Proxy {7}
//! Artifact Creature — Wizard, 4/3
//!
//! Prototype {1}{U}{U} — 2/1 (You may cast this spell with different mana cost, color, and size. It
//! keeps its abilities and types.)
//! When this creature enters, if you cast it, exile target instant or sorcery card with mana value
//! less than or equal to this creature's power from your graveyard. Copy that card. You may cast
//! the copy without paying its mana cost.
// End of the generated card header.

use crate::prelude::*;

/// "Target instant or sorcery card with mana value less than or equal to this creature's power
/// from your graveyard".
const SPELL_CARD: TargetSpec = TargetSpec::graveyard_card(Filter::All(&[
    instant_or_sorcery(),
    Filter::ManaValueAtMostPower,
    Filter::Whose(&Players::You),
]));

const PROXY: Face = Printed::ArcaneProxy.abilities(&[when_enters(&sequence(&[
    exile(target()),
    may_cast_copy_free(Players::You, those()),
]))
.target(&SPELL_CARD)
.only_if(&Condition::Cast(Objects::This))]);

pub static CARD: CardDef = card(Card::ArcaneProxy, &[PROXY, PROXY.prototyped()]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const PROXY: &str = Card::ArcaneProxy;
    /// A sorcery of mana value 4 that draws three cards.
    const FOUR: &str = Card::Concentrate;
    /// A sorcery of mana value 5.
    const FIVE: &str = Card::TimeWarp;
    /// A sorcery of mana value 2: its caster draws two cards and loses 2 life.
    const TWO: &str = Card::NightsWhisper;
    const ISLAND: &str = Card::Island;

    /// P0 casts Arcane Proxy prototyped, for {1}{U}{U}.
    const fn prototyped() -> Action {
        Action::Cast {
            player: P0,
            card: PROXY,
            face: FaceIx::PROTOTYPE,
            costs: Castings::only(0),
        }
    }

    pub const SCENARIOS: &[Scenario] = &[
        // Cast for {7}, the Proxy is a colorless 4/3, so its enters ability may copy a card of
        // mana value 4 but not 5. The copy is cast from exile, and the card stays exiled.
        Scenario::new("cast normally, a colorless 4/3 copies a card of mana value 4")
            .interaction(&[FOUR, FIVE])
            .battlefield(P0, &[ISLAND; 7])
            .hand(P0, &[PROXY])
            .graveyard(P0, &[FOUR, FIVE])
            .library(P0, &[ISLAND, ISLAND, ISLAND, ISLAND])
            .actions(&[
                ME.casts(PROXY),
                ME.colored(Zone::Stack, PROXY, Colors::NONE),
                ME.pt_in(Zone::Stack, PROXY, 4, 3),
                Resolve,
                Resolve,
                ME.deciding(DecisionKind::CastOffer, 2),
                ME.chooses(DecisionKind::CastOffer, FOUR),
                Resolve,
                ME.has(Zone::Hand, ISLAND, 3),
                ME.has(Zone::Exile, FOUR, 1),
                ME.has(Zone::Graveyard, FOUR, 0),
                ME.has(Zone::Graveyard, FIVE, 1),
                ME.pt(PROXY, 4, 3),
                ME.colored(Zone::Battlefield, PROXY, Colors::NONE),
            ]),
        // Cast prototyped for {1}{U}{U}, the Proxy is a blue 2/1 on the stack and on the
        // battlefield, so it may copy a card of mana value 2 but not 4.
        Scenario::new("cast prototyped, a blue 2/1 copies a card of mana value 2")
            .interaction(&[FOUR, TWO])
            .battlefield(P0, &[ISLAND; 3])
            .hand(P0, &[PROXY])
            .graveyard(P0, &[FOUR, TWO])
            .library(P0, &[ISLAND, ISLAND, ISLAND])
            .actions(&[
                prototyped(),
                ME.colored(Zone::Stack, PROXY, Colors::BLUE),
                ME.pt_in(Zone::Stack, PROXY, 2, 1),
                Resolve,
                Resolve,
                ME.chooses(DecisionKind::CastOffer, TWO),
                Resolve,
                ME.has(Zone::Hand, ISLAND, 2),
                ME.life(18),
                ME.has(Zone::Exile, TWO, 1),
                ME.has(Zone::Graveyard, FOUR, 1),
                ME.pt(PROXY, 2, 1),
                ME.colored(Zone::Battlefield, PROXY, Colors::BLUE),
            ]),
        // Ruling: "You cast the copy while the ability is resolving ... If you don't want to
        // cast the copy, you can choose not to; the copy ceases to exist". The card stays in
        // exile.
        Scenario::new("a copy not cast ceases to exist and the card stays exiled")
            .interaction(&[TWO])
            .battlefield(P0, &[ISLAND; 3])
            .hand(P0, &[PROXY])
            .graveyard(P0, &[TWO])
            .actions(&[
                prototyped(),
                Resolve,
                Resolve,
                ME.answers(DecisionKind::CastOffer, Pick::Pass),
                ME.has(Zone::Exile, TWO, 1),
                ME.has(Zone::Graveyard, TWO, 0),
                ME.has(Zone::Hand, TWO, 0),
                ME.life(20),
            ]),
        // Ruling: "A prototype card is a colorless card in every zone except the stack or the
        // battlefield". In hand the Proxy is not blue, so Force of Will cannot exile it to
        // pay its alternative cost: P0 has nothing to cast, and Lightning Bolt resolves with
        // no player asked.
        Scenario::new("in hand it is colorless and Force of Will cannot exile it")
            .interaction(&[Card::ForceOfWill, Card::LightningBolt])
            .hand(P0, &[PROXY, Card::ForceOfWill])
            .battlefield(P1, &[Card::Mountain])
            .hand(P1, &[Card::LightningBolt])
            .at(1, P1, TurnStep::PrecombatMain)
            .actions(&[
                ME.colored(Zone::Hand, PROXY, Colors::NONE),
                THEM.casts(Card::LightningBolt),
                THEM.targets_player(ME),
                ME.life(17),
                ME.has(Zone::Hand, Card::ForceOfWill, 1),
            ]),
        // Ruling: "If ... the permanent it becomes leaves the battlefield, it immediately
        // resumes using its normal characteristics." Flickerwisp exiles the prototyped Proxy,
        // which returns at the end step as a colorless 4/3. It was not cast, so its enters
        // ability does not trigger, though its power is now that of the card in the graveyard.
        Scenario::new("a blinked prototyped Proxy returns a 4/3 and does not trigger")
            .interaction(&[FOUR, Card::Flickerwisp])
            .battlefield(
                P0,
                &[
                    ISLAND,
                    ISLAND,
                    ISLAND,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .hand(P0, &[PROXY, Card::Flickerwisp])
            .graveyard(P0, &[FOUR])
            .actions(&[
                prototyped(),
                Resolve,
                ME.pt(PROXY, 2, 1),
                ME.casts(Card::Flickerwisp),
                Resolve,
                ME.targets(PROXY),
                Resolve,
                ME.has(Zone::Exile, PROXY, 1),
                advance_to(1, TurnStep::End),
                Resolve,
                ME.pt(PROXY, 4, 3),
                ME.colored(Zone::Battlefield, PROXY, Colors::NONE),
                ME.has(Zone::Graveyard, FOUR, 1),
                ME.has(Zone::Exile, FOUR, 0),
            ]),
        // Ruling: "if an effect ... causes another permanent to become a copy of it, the copy
        // would have the same characteristics as the prototyped permanent." The Proxy's own
        // trigger exiles one Night's Whisper, whose copy P0 declines. Phantasmal Image enters
        // as a blue 2/1 copy of the prototyped Proxy. It was cast, so its copied enters ability
        // triggers, and with power 2 it may copy the other Night's Whisper but not Concentrate.
        Scenario::new("a Phantasmal Image of a prototyped Proxy is a blue 2/1 that triggers")
            .interaction(&[TWO, FOUR, Card::PhantasmalImage])
            .battlefield(P0, &[ISLAND; 5])
            .hand(P0, &[PROXY, Card::PhantasmalImage])
            .graveyard(P0, &[FOUR, TWO, TWO])
            .library(P0, &[ISLAND, ISLAND, ISLAND])
            .actions(&[
                prototyped(),
                Resolve,
                Resolve,
                ME.answers(DecisionKind::CastOffer, Pick::Pass),
                ME.casts(Card::PhantasmalImage),
                Resolve,
                ME.picks_up_to(PROXY),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, TWO),
                Resolve,
                ME.controls(PROXY, 2),
                ME.life(18),
                ME.has(Zone::Hand, ISLAND, 2),
                ME.has(Zone::Graveyard, FOUR, 1),
                ME.has(Zone::Exile, TWO, 2),
            ]),
        // Ruling: "If another effect causes Arcane Proxy's power to be less than the mana value
        // of the target card as the ability tries to resolve, the target is illegal." Twisted
        // Image makes the prototyped 2/1 a 1/2 in response, so Night's Whisper is no longer a
        // legal target: it stays in the graveyard and no copy is offered.
        Scenario::new("a power lowered in response makes the target illegal")
            .interaction(&[TWO, Card::TwistedImage])
            .battlefield(P0, &[ISLAND; 4])
            .hand(P0, &[PROXY, Card::TwistedImage])
            .graveyard(P0, &[TWO])
            .library(P0, &[ISLAND, ISLAND])
            .actions(&[
                prototyped(),
                Resolve,
                ME.casts(Card::TwistedImage),
                Resolve,
                ME.pt(PROXY, 1, 2),
                Resolve,
                ME.has(Zone::Graveyard, TWO, 1),
                ME.has(Zone::Exile, TWO, 0),
                ME.life(20),
                stack(0),
            ]),
        // Ruling: "Casting a prototyped spell isn't the same as casting it for an alternative
        // cost, and an alternative cost may be applied to a spell cast this way." Possibility
        // Storm lets P0 cast the Proxy from exile without paying its mana cost, and P0 casts it
        // prototyped: it is a blue 2/1, and no land was tapped.
        Scenario::new("cast prototyped from exile without paying its mana cost")
            .interaction(&[Card::PossibilityStorm, Card::Ornithopter])
            .battlefield(P0, &[Card::PossibilityStorm, ISLAND])
            .hand(P0, &[Card::Ornithopter])
            .library(P0, &[ISLAND, PROXY, Card::Mountain])
            .actions(&[
                ME.casts(Card::Ornithopter),
                Resolve,
                ME.answers(
                    DecisionKind::CastOffer,
                    Pick::Cast {
                        card: PROXY,
                        face: FaceIx::PROTOTYPE,
                        costs: Castings::only(0),
                    },
                ),
                Resolve,
                ME.pt(PROXY, 2, 1),
                ME.colored(Zone::Battlefield, PROXY, Colors::BLUE),
                ME.tapped(ISLAND, 0),
            ]),
        // Ruling: "If the spell you cast has {X} in its mana cost, you must choose 0 as the value
        // of X." The copy of Braingeyser, mana value 2, is cast for nothing with X 0, so its
        // target draws no card.
        Scenario::new("a copy cast without paying its mana cost has X 0")
            .interaction(&[Card::Braingeyser])
            .battlefield(P0, &[ISLAND; 3])
            .hand(P0, &[PROXY])
            .graveyard(P0, &[Card::Braingeyser])
            .library(P0, &[ISLAND, ISLAND, ISLAND])
            .actions(&[
                prototyped(),
                Resolve,
                Resolve,
                ME.chooses(DecisionKind::CastOffer, Card::Braingeyser),
                ME.targets_player(ME),
                Resolve,
                ME.has_cards(Zone::Hand, 0),
                ME.has(Zone::Exile, Card::Braingeyser, 1),
            ]),
    ];
}
