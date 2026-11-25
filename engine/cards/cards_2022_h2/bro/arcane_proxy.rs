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

static SPELL_IN_GRAVEYARD: &[Filter] = &[
    Filter::Types(Types::INSTANT.union(Types::SORCERY)),
    Filter::ManaValueAtMostSourcePower,
    Filter::Whose(&Players::You),
];

static COPY_AND_CAST: Effect = sequence(&[
    exile(target()),
    copy_card(those()),
    may_cast_free(you(), those()),
]);

pub static CARD: CardDef = Printed::ArcaneProxy
    .abilities(&[when_enters(&COPY_AND_CAST)
        .only_if(&Condition::Cast)
        .target(&TargetSpec::graveyard_card(Filter::All(SPELL_IN_GRAVEYARD)))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const PROXY: &str = Card::ArcaneProxy;

    pub const SCENARIOS: &[Scenario] = &[
        // Cast normally for {7}: 4/3 colorless, can target and copy a mana value 4 sorcery.
        Scenario::new("Arcane Proxy cast normally copies a mana value 4 card")
            .interaction(&[Card::TendrilsOfAgony])
            .hand(P0, &[PROXY])
            .battlefield(P0, &[Card::Plains; 7])
            .graveyard(P0, &[Card::TendrilsOfAgony])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(PROXY),
                ME.colored(Zone::Stack, PROXY, Colors::NONE),
                Resolve,
                ME.pt(PROXY, 4, 3),
                ME.colored(Zone::Battlefield, PROXY, Colors::NONE),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, Card::TendrilsOfAgony),
                ME.targets_player(THEM),
                Resolve,
                ME.targets_player(THEM),
                Resolve,
                Resolve,
                THEM.life(16),
                ME.life(24),
                ME.has(Zone::Exile, Card::TendrilsOfAgony, 1),
            ]),
        // Cast prototyped for {1}{U}{U}: 2/1 blue; only MV <= 2 cards are legal, not MV 4.
        Scenario::new("Arcane Proxy cast prototyped copies a mana value 2 or less card")
            .interaction(&[Card::LightningBolt, Card::TendrilsOfAgony])
            .hand(P0, &[PROXY])
            .battlefield(P0, &[Card::Island; 3])
            .graveyard(
                P0,
                &[Card::LightningBolt, Prop::Sorcery, Card::TendrilsOfAgony],
            )
            .library(P0, &[Card::Island; 4])
            .library(P1, &[Card::Plains])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: PROXY,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.colored(Zone::Stack, PROXY, Colors::BLUE),
                Resolve,
                ME.pt(PROXY, 2, 1),
                ME.colored(Zone::Battlefield, PROXY, Colors::BLUE),
                ME.deciding(DecisionKind::Target, 2),
                ME.targets(Card::LightningBolt),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, Card::LightningBolt),
                ME.targets_player(THEM),
                Resolve,
                THEM.life(17),
                ME.has(Zone::Exile, Card::LightningBolt, 1),
            ]),
        // Declining to cast the copy leaves the exiled card in exile and lets the copy cease
        // to exist at the next state-based check (CR 704.5e).
        Scenario::new("declining to cast the copy lets it cease to exist")
            .hand(P0, &[PROXY])
            .battlefield(P0, &[Card::Island; 3])
            .graveyard(P0, &[Prop::Sorcery])
            .battlefield(P1, &[Prop::Vanilla])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: PROXY,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Resolve,
                Resolve,
                ME.answers(DecisionKind::CastOffer, Pick::Pass),
                stack(0),
                ME.has(Zone::Exile, Prop::Sorcery, 1),
                ME.has_cards(Zone::Exile, 1),
                THEM.controls(Prop::Vanilla, 1),
            ]),
        // In hand Arcane Proxy is colorless and cannot be exiled for Force of Will; cast
        // prototyped it is blue on the stack and battlefield, and protection from blue stops it.
        Scenario::new("prototyped Proxy is blue for protection while hand copy is colorless")
            .interaction(&[Card::ForceOfWill, Card::MotherOfRunes])
            .at(2, P1, TurnStep::PrecombatMain)
            .hand(P0, &[PROXY, Card::ForceOfWill, Prop::Removal])
            .battlefield(P0, &[Card::Island, Card::Island, Card::Island])
            .hand(P1, &[PROXY])
            .battlefield(
                P1,
                &[
                    Card::Plains,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::MotherOfRunes,
                    Prop::Vanilla,
                ],
            )
            .library(P0, &[Card::Island; 3])
            .library(P1, &[Card::Plains; 3])
            .actions(&[
                ME.colored(Zone::Hand, PROXY, Colors::NONE),
                THEM.colored(Zone::Hand, PROXY, Colors::NONE),
                Action::Cast {
                    player: P1,
                    card: PROXY,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                THEM.colored(Zone::Stack, PROXY, Colors::BLUE),
                THEM.passes(),
                // Force of Will exiles Prop::Removal (the only blue card in P0's hand, auto-picked),
                // not P0's colorless Arcane Proxy.
                Action::Cast {
                    player: P0,
                    card: Card::ForceOfWill,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.has(Zone::Exile, Prop::Removal, 1),
                ME.has(Zone::Hand, PROXY, 1),
                Resolve,
                THEM.has(Zone::Graveyard, PROXY, 1),
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                THEM.done_attacking(),
                advance_to(3, TurnStep::PrecombatMain),
                Action::Cast {
                    player: P0,
                    card: PROXY,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Resolve,
                ME.colored(Zone::Battlefield, PROXY, Colors::BLUE),
                advance_to(4, TurnStep::BeginningOfCombat),
                THEM.activates(Card::MotherOfRunes, 0),
                THEM.targets(Prop::Vanilla),
                Resolve,
                THEM.chooses_color(Colors::BLUE),
                THEM.attacks_with(Prop::Vanilla),
                // Prototyped Proxy is blue and cannot block a creature with protection from blue.
                advance_to(4, TurnStep::PostcombatMain),
                ME.life(17),
            ]),
        // Blinked by Flickerwisp, prototyped Proxy returns as a normal 4/3 colorless creature
        // and does not trigger its enters ability (it was not cast).
        Scenario::new("blinked Arcane Proxy returns unprototyped and does not trigger")
            .interaction(&[Card::Flickerwisp, Card::LightningBolt])
            .hand(P0, &[PROXY, Card::Flickerwisp])
            .battlefield(
                P0,
                &[
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .graveyard(P0, &[Card::LightningBolt, Prop::Sorcery])
            .battlefield(P1, &[Prop::Vanilla])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Plains])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: PROXY,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Resolve,
                ME.targets(Card::LightningBolt),
                Resolve,
                ME.answers(DecisionKind::CastOffer, Pick::Pass),
                ME.pt(PROXY, 2, 1),
                ME.colored(Zone::Battlefield, PROXY, Colors::BLUE),
                ME.casts(Card::Flickerwisp),
                Resolve,
                ME.targets(PROXY),
                Resolve,
                advance_to(1, TurnStep::End),
                Resolve,
                stack(0),
                ME.pt(PROXY, 4, 3),
                ME.colored(Zone::Battlefield, PROXY, Colors::NONE),
            ]),
        // Phantasmal Image copying a prototyped Proxy has 2/1 blue stats and triggers its
        // copied enters ability because Phantasmal Image was cast.
        Scenario::new("Phantasmal Image of a prototyped Proxy is 2/1 blue and triggers")
            .interaction(&[Card::PhantasmalImage, Card::LightningBolt])
            .hand(P0, &[PROXY, Card::PhantasmalImage])
            .battlefield(P0, &[Card::Island; 5])
            .graveyard(P0, &[Card::LightningBolt, Card::LightningBolt])
            .battlefield(P1, &[Prop::Vanilla])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Plains])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: PROXY,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Resolve,
                Resolve,
                ME.answers(DecisionKind::CastOffer, Pick::Pass),
                ME.casts(Card::PhantasmalImage),
                Resolve,
                ME.picks_up_to(PROXY),
                // The Image becomes a 2/1 blue copy named Arcane Proxy and triggers on entry.
                Resolve,
                ME.chooses(DecisionKind::CastOffer, Card::LightningBolt),
                ME.targets_player(THEM),
                Resolve,
                ME.controls(PROXY, 2),
                ME.has_subtype(PROXY, Subtype::Illusion, true),
                THEM.life(17),
            ]),
        // If Proxy's power is reduced below the target's mana value before resolution, the
        // target becomes illegal and nothing is exiled (CR 608.2b).
        Scenario::new("power reduction before resolution makes the target illegal")
            .interaction(&[Card::GloriousAnthem, Card::GetLost, Card::LightningBolt])
            .hand(P0, &[PROXY, Card::LightningBolt])
            .battlefield(
                P0,
                &[
                    Card::GloriousAnthem,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Mountain,
                ],
            )
            .graveyard(P0, &[Prop::UntargetedSpell])
            .hand(P1, &[Card::GetLost])
            .battlefield(P1, &[Card::Plains, Card::Plains])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Plains])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: PROXY,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Resolve,
                ME.pt(PROXY, 3, 2),
                ME.passes(),
                THEM.casts(Card::GetLost),
                THEM.targets(Card::GloriousAnthem),
                Resolve,
                ME.pt(PROXY, 2, 1),
                Resolve,
                stack(0),
                ME.has(Zone::Graveyard, Prop::UntargetedSpell, 1),
            ]),
        // If Proxy leaves the battlefield before resolution, its last known power is used.
        Scenario::new("leaving the battlefield before resolution uses last known power")
            .interaction(&[Card::GetLost, Card::LightningBolt])
            .hand(P0, &[PROXY, Card::LightningBolt])
            .battlefield(
                P0,
                &[Card::Island, Card::Island, Card::Island, Card::Mountain],
            )
            .graveyard(P0, &[Card::LightningBolt])
            .hand(P1, &[Card::GetLost])
            .battlefield(P1, &[Card::Plains, Card::Plains, Prop::Vanilla])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Plains])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: PROXY,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Resolve,
                ME.passes(),
                THEM.casts(Card::GetLost),
                THEM.targets(PROXY),
                Resolve,
                ME.has(Zone::Graveyard, PROXY, 1),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, Card::LightningBolt),
                ME.targets_player(THEM),
                Resolve,
                THEM.life(17),
            ]),
    ];
}
