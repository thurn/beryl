// Generated from the card store by `just card header`: notes go below the end marker.
//! Underworld Breach {1}{R}
//! Enchantment
//!
//! Each nonland card in your graveyard has escape. The escape cost is equal to the card's mana cost
//! plus exile three other cards from your graveyard. (You may cast cards from your graveyard for
//! their escape cost.)
//! At the beginning of the end step, sacrifice this enchantment.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::UnderworldBreach
    .abilities(&[
        Ability::static_ability(&LayerSpec {
            affected: Objects::Everywhere(Filter::All(&[
                nonland(),
                yours(),
                Filter::InZone(Zone::Graveyard),
            ])),
            condition: None,
            modifications: &[Modification::GrantEscape],
        }),
        Ability::triggered(&Trigger::step(TurnStep::End, Players::EachPlayer))
            .effect(&sacrifice(this())),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const BREACH: &str = Card::UnderworldBreach;
    const LED: &str = Card::LionsEyeDiamond;
    const SNAPCASTER: &str = Card::SnapcasterMage;
    const FREEZE: &str = Card::BrainFreeze;

    /// Lion's Eye Diamond, sacrificed for {U}{U}{U}.
    const BLUE: [Action; 2] = [ME.activates(LED, 0), ME.chooses_color(Colors::BLUE)];

    /// Escapes Lion's Eye Diamond and lets it resolve. Of the other cards in the graveyard,
    /// Brain Freeze lies on top, so P0 picks an Island first and the rest follow.
    const ESCAPE_LED: [Action; 3] = [ME.escapes(LED), ME.picks(Card::Island), Resolve];

    /// The Breach loop (decision R13 builds no shortcut): Lion's Eye Diamond and Brain Freeze
    /// cast, then escaped in turn, the Diamond sacrificed for {U}{U}{U} with each Brain
    /// Freeze on the stack. Each round exiles six cards of the graveyard, returns the two
    /// spells to it, gains one mana, and its Brain Freeze has two more copies than the last.
    /// Rounds 1 and 2 mill P0 (six and twelve cards), round 3 two copies' worth of P0 and
    /// four of P1 (twelve), and rounds 4 and 5 only P1 (twenty-four, then the last
    /// seventeen of thirty). The whole turn stays inside the decision budget the scenario
    /// runs under (`Limits::DEFAULT`, 500 decisions a turn).
    const LOOP: &[Action] = &[
        // Round 1: storm 1.
        ME.casts(LED),
        Resolve,
        ME.casts(FREEZE),
        ME.targets_player(ME),
        BLUE[0],
        BLUE[1],
        Resolve,
        ME.targets_player(ME),
        Resolve,
        Resolve,
        ME.has_cards(Zone::Graveyard, 8),
        ME.mana(ManaColor::Blue, 3),
        // Round 2: storm 3.
        ESCAPE_LED[0],
        ESCAPE_LED[1],
        ESCAPE_LED[2],
        ME.escapes(FREEZE),
        ME.targets_player(ME),
        BLUE[0],
        BLUE[1],
        Resolve,
        ME.targets_player(ME),
        ME.targets_player(ME),
        ME.targets_player(ME),
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        ME.has_cards(Zone::Graveyard, 14),
        ME.mana(ManaColor::Blue, 4),
        // Round 3: storm 5.
        ESCAPE_LED[0],
        ESCAPE_LED[1],
        ESCAPE_LED[2],
        ME.escapes(FREEZE),
        ME.targets_player(THEM),
        BLUE[0],
        BLUE[1],
        Resolve,
        ME.targets_player(ME),
        ME.targets_player(ME),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        THEM.has_cards(Zone::Library, 41),
        ME.has_cards(Zone::Graveyard, 14),
        ME.mana(ManaColor::Blue, 5),
        // Round 4: storm 7.
        ESCAPE_LED[0],
        ESCAPE_LED[1],
        ESCAPE_LED[2],
        ME.escapes(FREEZE),
        ME.targets_player(THEM),
        BLUE[0],
        BLUE[1],
        Resolve,
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        THEM.has_cards(Zone::Library, 17),
        ME.has_cards(Zone::Graveyard, 8),
        ME.mana(ManaColor::Blue, 6),
        // Round 5: storm 9, and P1's library is empty.
        ESCAPE_LED[0],
        ESCAPE_LED[1],
        ESCAPE_LED[2],
        ME.escapes(FREEZE),
        ME.targets_player(THEM),
        Resolve,
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        THEM.has_cards(Zone::Library, 0),
        ME.mana(ManaColor::Blue, 4),
    ];

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("a creature sacrificed for an escaped spell pays its exile cost")
            .interaction(&[Card::VillageRites])
            .battlefield(P0, &[BREACH, Card::Swamp, Prop::Vanilla])
            .graveyard(P0, &[Card::VillageRites, Card::Island, Card::Plains])
            .battlefield(P1, &[Card::Island; 2])
            .hand(P1, &[Prop::Instant])
            .actions(&[
                ME.escapes(Card::VillageRites),
                ME.tapped(Card::Swamp, 1),
                ME.controls(Prop::Vanilla, 0),
                ME.has(Zone::Exile, Prop::Vanilla, 1),
                ME.has(Zone::Exile, Card::Island, 1),
                ME.has(Zone::Exile, Card::Plains, 1),
                ME.has_cards(Zone::Graveyard, 0),
                THEM.deciding(DecisionKind::Priority, 2),
                THEM.casts(Prop::Instant),
                ME.has(Zone::Graveyard, Card::VillageRites, 1),
            ]),
        Scenario::new("a card discarded for an escaped spell pays its exile cost")
            .interaction(&[Card::ThrillOfPossibility])
            .battlefield(P0, &[BREACH, Card::Mountain, Card::Swamp])
            .hand(P0, &[Card::Forest])
            .graveyard(P0, &[Card::ThrillOfPossibility, Card::Island, Card::Plains])
            .battlefield(P1, &[Card::Island; 2])
            .hand(P1, &[Prop::Instant])
            .actions(&[
                ME.escapes(Card::ThrillOfPossibility),
                ME.tapped(Card::Mountain, 1),
                ME.tapped(Card::Swamp, 1),
                ME.has_cards(Zone::Hand, 0),
                ME.has(Zone::Exile, Card::Forest, 1),
                ME.has(Zone::Exile, Card::Island, 1),
                ME.has(Zone::Exile, Card::Plains, 1),
                ME.has_cards(Zone::Graveyard, 0),
                THEM.deciding(DecisionKind::Priority, 2),
                THEM.casts(Prop::Instant),
                ME.has(Zone::Graveyard, Card::ThrillOfPossibility, 1),
            ]),
        Scenario::new("a sacrificed token cannot supply a card for escape")
            .interaction(&[Card::VillageRites, Card::AdelineResplendentCathar])
            .battlefield(P0, &[BREACH, Card::Swamp, Card::AdelineResplendentCathar])
            .battlefield(P1, &[Card::Island; 3])
            .hand(P0, &[Card::Island])
            .hand(P1, &[Prop::Removal])
            .graveyard(P0, &[Card::VillageRites, Card::Island, Card::Plains])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                THEM.passes(),
                ME.attacks_with(Card::AdelineResplendentCathar),
                Resolve,
                ME.controls(crate::tokens::HUMAN_TOKEN, 1),
                ME.passes(),
                THEM.casts(Prop::Removal),
                THEM.targets(Card::AdelineResplendentCathar),
                Resolve,
                ME.controls(Card::AdelineResplendentCathar, 0),
                advance_to(1, TurnStep::PostcombatMain),
                ME.deciding(DecisionKind::Priority, 2),
            ]),
        Scenario::new("a sacrifice moved to exile cannot supply a card for escape")
            .interaction(&[Card::VillageRites, Card::RestInPeace])
            .battlefield(P0, &[BREACH, Card::Swamp, Prop::Vanilla])
            .battlefield(P1, &[Card::RestInPeace])
            .hand(P0, &[Card::Island])
            .graveyard(P0, &[Card::VillageRites, Card::Island, Card::Plains])
            .actions(&[ME.deciding(DecisionKind::Priority, 2)]),
        Scenario::new("the Breach loop empties a 53-card library")
            .interaction(&[LED, FREEZE])
            .battlefield(P0, &[BREACH, Card::Island, Card::Island])
            .hand(P0, &[LED, FREEZE])
            .library(P0, &[Card::Island; 30])
            .library(P1, &[Card::Plains; 53])
            .actions(LOOP),
        // The creature enters after it escapes and returns to the graveyard when it dies.
        Scenario::new("a creature card escapes and enters the battlefield")
            .interaction(&[Card::LightningBolt])
            .battlefield(P0, &[BREACH, Card::Island, Card::Island])
            .battlefield(P1, &[Card::Mountain])
            .hand(P1, &[Card::LightningBolt])
            .graveyard(
                P0,
                &[Prop::Vanilla, Card::Plains, Card::Plains, Card::Plains],
            )
            .actions(&[
                ME.escapes(Prop::Vanilla),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                ME.has(Zone::Exile, Card::Plains, 3),
                ME.tapped(Card::Island, 2),
                ME.has_cards(Zone::Graveyard, 0),
                ME.passes(),
                THEM.casts(Card::LightningBolt),
                THEM.targets(Prop::Vanilla),
                ME.has(Zone::Graveyard, Prop::Vanilla, 1),
            ]),
        // Ruling: "After an escaped spell resolves, it returns to its owner's graveyard if
        // it's not a permanent spell." With four other cards, P0 chooses the three to exile,
        // one at a time in the graveyard's order, top first: the Swamp, then an Island, and the
        // last Island is the only choice left. The Plains stays.
        Scenario::new("an escaped sorcery returns to the graveyard")
            .battlefield(P0, &[BREACH, Card::Plains])
            .graveyard(
                P0,
                &[
                    Prop::Sorcery,
                    Card::Island,
                    Card::Island,
                    Card::Plains,
                    Card::Swamp,
                ],
            )
            .battlefield(P1, &[Prop::Vanilla])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.escapes(Prop::Sorcery),
                ME.picks(Card::Swamp),
                ME.picks(Card::Island),
                stack(1),
                Resolve,
                THEM.controls(Prop::Vanilla, 0),
                ME.has(Zone::Graveyard, Prop::Sorcery, 1),
                ME.has(Zone::Graveyard, Card::Plains, 1),
                ME.has(Zone::Exile, Card::Island, 2),
                ME.has(Zone::Exile, Card::Swamp, 1),
            ]),
        // Ruling: "Escape's permission doesn't change when you may cast the spell from your
        // graveyard." On P1's turn, P0 escapes an instant in response to P1's creature
        // spell, and not the sorcery beside it.
        Scenario::new("an instant escapes at instant speed, a sorcery doesn't")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[BREACH, Card::Island, Card::Island, Card::Plains])
            .graveyard(
                P0,
                &[
                    Prop::Instant,
                    Prop::Sorcery,
                    Card::Swamp,
                    Card::Swamp,
                    Card::Swamp,
                ],
            )
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Prop::Vanilla])
            .actions(&[
                THEM.casts(Prop::Vanilla),
                ME.deciding(DecisionKind::Priority, 2),
                ME.escapes(Prop::Instant),
                // The Swamps, not Oust.
                ME.picks(Card::Swamp),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
                ME.has(Zone::Graveyard, Prop::Instant, 1),
            ]),
        // Ruling: "If a card has multiple abilities giving you permission to cast it, such as
        // ... an escape ability and a flashback ability, you choose which one to apply. The
        // others have no effect." Snapcaster gives Oust flashback; P0 may cast it with
        // flashback or with escape, and escapes it, so it returns to the graveyard rather
        // than being exiled as a flashback spell would be.
        Scenario::new("escape or flashback: the one chosen applies")
            .interaction(&[SNAPCASTER])
            .battlefield(P0, &[BREACH, Card::Island, Card::Island, Card::Plains])
            .hand(P0, &[SNAPCASTER])
            .graveyard(P0, &[Prop::Sorcery, Card::Swamp, Card::Swamp, Card::Swamp])
            .battlefield(P1, &[Prop::Vanilla])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.casts(SNAPCASTER),
                Resolve,
                Resolve,
                // Passing, Oust with flashback and Oust with escape.
                ME.deciding(DecisionKind::Priority, 3),
                ME.escapes(Prop::Sorcery),
                ME.targets(Prop::Vanilla),
                Resolve,
                THEM.controls(Prop::Vanilla, 0),
                ME.has(Zone::Graveyard, Prop::Sorcery, 1),
                ME.has(Zone::Exile, Prop::Sorcery, 0),
            ]),
        // Ruling: "If you cast a spell with its escape permission, you can't choose to apply
        // any other alternative costs". Force of Will's own alternative cost (a blue card from
        // hand and 1 life) can't be added to its escape: P0 may pass, cast Counterspell from
        // their hand, or escape Force of Will for {3}{U}{U}.
        Scenario::new("escape is the one alternative cost")
            .interaction(&[Card::ForceOfWill])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(
                P0,
                &[
                    BREACH,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .hand(P0, &[Card::Counterspell])
            .graveyard(
                P0,
                &[Card::ForceOfWill, Card::Plains, Card::Plains, Card::Plains],
            )
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Prop::Vanilla])
            .actions(&[
                THEM.casts(Prop::Vanilla),
                ME.deciding(DecisionKind::Priority, 3),
                ME.escapes(Card::ForceOfWill),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
                ME.has(Zone::Hand, Card::Counterspell, 1),
                ME.life(20),
            ]),
        // Ruling: "To determine the total cost of a spell, start with the mana cost or
        // alternative cost you're paying (such as an escape cost), add any cost increases".
        // Under Trinisphere, Lion's Eye Diamond's escape cost of {0} becomes {3}.
        Scenario::new("a cost increase applies to the escape cost")
            .interaction(&[Card::Trinisphere, LED])
            .battlefield(
                P0,
                &[
                    BREACH,
                    Card::Trinisphere,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .graveyard(P0, &[LED, Card::Plains, Card::Plains, Card::Plains])
            .actions(&[
                ME.escapes(LED),
                Resolve,
                ME.controls(LED, 1),
                ME.tapped(Card::Island, 3),
            ]),
        // Escape's cost is the card's mana cost, {6}{B} for Gurmag Angler, which delve can
        // pay part of, plus three other cards. Delve exiles its cards first, so it may take
        // only three of the six Islands, leaving three for escape; the four Swamps pay
        // {3}{B}. The player pays the total cost (CR 601.2h).
        Scenario::new("a delve card escapes, delving only what escape leaves")
            .interaction(&[Card::GurmagAngler])
            .battlefield(
                P0,
                &[BREACH, Card::Swamp, Card::Swamp, Card::Swamp, Card::Swamp],
            )
            .graveyard(
                P0,
                &[
                    Card::GurmagAngler,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .actions(&[
                ME.escapes(Card::GurmagAngler),
                Resolve,
                ME.controls(Card::GurmagAngler, 1),
                ME.has(Zone::Exile, Card::Island, 6),
                ME.tapped(Card::Swamp, 4),
            ]),
        // With three Swamps, Gurmag Angler's escape needs four cards for delve and three for
        // escape, seven of the six Islands, so it can't be cast: P0 may only pass or play the
        // Island in their hand.
        Scenario::new("delve and escape can't exile the same cards")
            .interaction(&[Card::GurmagAngler])
            .battlefield(P0, &[BREACH, Card::Swamp, Card::Swamp, Card::Swamp])
            .hand(P0, &[Card::Island])
            .graveyard(
                P0,
                &[
                    Card::GurmagAngler,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .actions(&[ME.deciding(DecisionKind::Priority, 2)]),
        // Abhorrent Oculus's escape takes its additional cost, six cards from the graveyard,
        // and escape's three other cards: nine of the eight Plains, so it can't be cast
        // (CR 601.2h). P0 may only pass or play the Island in their hand.
        Scenario::new("an additional cost and escape can't exile the same cards")
            .interaction(&[Card::AbhorrentOculus])
            .battlefield(P0, &[BREACH, Card::Island, Card::Island, Card::Island])
            .hand(P0, &[Card::Island])
            .graveyard(
                P0,
                &[
                    Card::AbhorrentOculus,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .actions(&[ME.deciding(DecisionKind::Priority, 2)]),
        // With nine, it escapes: the six and the three are all exiled.
        Scenario::new("an additional cost and escape exile their cards together")
            .interaction(&[Card::AbhorrentOculus])
            .battlefield(P0, &[BREACH, Card::Island, Card::Island, Card::Island])
            .graveyard(
                P0,
                &[
                    Card::AbhorrentOculus,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .actions(&[
                ME.escapes(Card::AbhorrentOculus),
                Resolve,
                ME.controls(Card::AbhorrentOculus, 1),
                ME.has(Zone::Exile, Card::Plains, 9),
            ]),
        // [CR 107.3a]: the caster "chooses and announces the value of X" as the spell is cast, and an
        // escaped spell's cost is its mana cost, so {X}{U}{U} with X = 2 is four mana.
        //
        // [CR 107.3a]: https://yawgatog.com/resources/magic-rules/#R1073a
        Scenario::new("an X spell escapes with X chosen")
            .interaction(&[Card::Braingeyser])
            .battlefield(
                P0,
                &[
                    BREACH,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .graveyard(
                P0,
                &[Card::Braingeyser, Card::Plains, Card::Plains, Card::Plains],
            )
            .library(P0, &[Card::Swamp; 4])
            .actions(&[
                ME.escapes(Card::Braingeyser),
                ME.x(2),
                ME.targets_player(ME),
                Resolve,
                ME.has(Zone::Hand, Card::Swamp, 2),
                ME.has(Zone::Exile, Card::Plains, 3),
                ME.tapped(Card::Island, 4),
            ]),
        // An Aura card that escapes is cast targeting a creature and enters attached to it
        // ([CR 303.4f]).
        //
        // [CR 303.4f]: https://yawgatog.com/resources/magic-rules/#R3034f
        Scenario::new("an Aura card escapes and enters attached")
            .interaction(&[Card::HolyStrength])
            .battlefield(P0, &[BREACH, Card::Plains, Prop::Vanilla])
            .graveyard(
                P0,
                &[Card::HolyStrength, Card::Island, Card::Island, Card::Island],
            )
            .actions(&[
                ME.escapes(Card::HolyStrength),
                Resolve,
                ME.pt(Prop::Vanilla, 3, 3),
                ME.has(Zone::Battlefield, Card::HolyStrength, 1),
                ME.has(Zone::Exile, Card::Island, 3),
            ]),
        // The Helm reduces the mana paid for escape to two, but Mana Drain derives
        // its mana from the spell's printed mana cost.
        Scenario::new("a reduced escape cost leaves the spell's mana value unchanged")
            .interaction(&[Card::HelmOfAwakening, Card::ManaDrain])
            .battlefield(
                P0,
                &[BREACH, Card::HelmOfAwakening, Card::Island, Card::Island],
            )
            .graveyard(
                P0,
                &[
                    Prop::UntargetedSpell,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .battlefield(P1, &[Card::Island; 2])
            .hand(P1, &[Card::ManaDrain, Card::Plains])
            .actions(&[
                ME.escapes(Prop::UntargetedSpell),
                ME.tapped(Card::Island, 2),
                THEM.casts(Card::ManaDrain),
                ME.has(Zone::Graveyard, Prop::UntargetedSpell, 1),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.passes(),
                THEM.mana(ManaColor::Colorless, 3),
            ]),
        // The active player can cast Lion's Eye Diamond from the graveyard as soon as
        // its mana ability finishes, before the opponent can cast Lightning Bolt.
        Scenario::new("a sacrificed Diamond can escape before the opponent gets priority")
            .interaction(&[LED, Card::LightningBolt])
            .battlefield(P0, &[BREACH, LED])
            .hand(P0, &[Card::Plains; 3])
            .battlefield(P1, &[Card::Mountain])
            .hand(P1, &[Card::LightningBolt])
            .actions(&[
                BLUE[0],
                BLUE[1],
                ME.escapes(LED),
                ME.has(Zone::Graveyard, LED, 0),
                ME.has(Zone::Exile, Card::Plains, 3),
                stack(1),
            ]),
    ];
}
