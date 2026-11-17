// Generated from the card store by `just card header`: notes go below the end marker.
//! Teferi's Protection {2}{W}
//! Instant
//!
//! Until your next turn, your life total can't change and you gain protection from everything. All
//! permanents you control phase out. (While they're phased out, they're treated as though they
//! don't exist. They phase in before you untap during your untap step.)
//! Exile Teferi's Protection.
// End of the generated card header.

use crate::prelude::*;

/// The instructions in order ([CR 608.2c]): "The controller of the spell or ability follows its
/// instructions in the order written."
///
/// "Your life total can't change" and "you gain protection from everything" are rule changes on
/// the controller ([CR 613.11]: "Some continuous effects affect game rules rather than
/// objects."), lasting until their next turn begins. While the first is in force they neither
/// gain nor lose life, and can't pay life ([CR 119.8]): "a cost that involves having that player
/// pay life can't be paid." The second is [CR 702.16j]: "A permanent or player with protection
/// from everything ... can't be targeted by spells or abilities and can't be enchanted by Auras.
/// ... All damage that would be dealt to such a permanent or player is prevented."
///
/// "All permanents you control phase out" ([CR 702.26b]): each is treated as though it does not
/// exist until its controller's next untap step ([CR 502.1]), and what is attached to it phases
/// out with it ([CR 702.26g]).
///
/// Exiled last, Teferi's Protection is not put into a graveyard as the final part of its
/// resolution: it is no longer on the stack.
///
/// [CR 608.2c]: https://yawgatog.com/resources/magic-rules/#R6082c
/// [CR 613.11]: https://yawgatog.com/resources/magic-rules/#R61311
/// [CR 119.8]: https://yawgatog.com/resources/magic-rules/#R1198
/// [CR 702.16j]: https://yawgatog.com/resources/magic-rules/#R70216j
/// [CR 702.26b]: https://yawgatog.com/resources/magic-rules/#R70226b
/// [CR 502.1]: https://yawgatog.com/resources/magic-rules/#R5021
/// [CR 702.26g]: https://yawgatog.com/resources/magic-rules/#R70226g
pub static CARD: CardDef = Printed::TeferisProtection
    .abilities(&[Ability::spell().effect(&sequence(&[
        change_rule(
            &RuleChange {
                players: Players::You,
                what: Governed::Player(
                    PlayerRules::LIFE_CANT_CHANGE.union(PlayerRules::PROTECTION_FROM_EVERYTHING),
                ),
                when: When::AsUsual,
            },
            Duration::UntilYourNextTurn,
        ),
        phase_out(Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::Whose(&Players::You),
        }),
        exile(this()),
    ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const PROTECTION: &str = Card::TeferisProtection;
    const BOLT: &str = Card::LightningBolt;
    const SCIMITAR: &str = Prop::Equipment;
    const IMAGE: &str = Card::PhantasmalImage;
    const HOLY: &str = Card::HolyStrength;
    const SKYDIVER: &str = Card::ThievingSkydiver;

    // `Prop::Flier` is a white 3/1 flier, `Prop::Vanilla` a blue 2/1 without flying.

    /// In P1's turn 2, P0 holds Teferi's Protection with three Plains to cast it and controls a
    /// blue 2/1; P1 holds Lightning Bolt with a Mountain and controls a white 3/1 flier.
    const fn in_their_turn(name: &'static str) -> Scenario {
        Scenario::new(name)
            .interaction(&[BOLT])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(
                P0,
                &[Card::Plains, Card::Plains, Card::Plains, Prop::Vanilla],
            )
            .hand(P0, &[PROTECTION])
            .battlefield(P1, &[Card::Mountain, Prop::Flier])
            .hand(P1, &[BOLT])
            .library(P0, &[Card::Plains; 3])
            .library(P1, &[Card::Plains; 3])
    }

    pub const SCENARIOS: &[Scenario] = &[
        // [CR 702.26b]: "a phased-out permanent is treated as though it does not exist. It can’t
        // affect or be affected by anything else in the game." P0's Phantasmal Image, a copy of
        // P1's blue 2/1, becomes the target of P0's equip ability, and its "sacrifice it" trigger
        // goes on the stack. Teferi's Protection phases the Image out in response, so the trigger
        // sacrifices nothing and the equip ability's one target is illegal. The Image phases in
        // at P0's next untap step, still a 2/1 and not equipped.
        //
        // [CR 702.26b]: https://yawgatog.com/resources/magic-rules/#R70226b
        // [CR 702.26i]: "An Aura, Equipment, or Fortification that phased out directly will phase
        // in attached to the object or player it was attached to when it phased out, if that
        // object is still in the same zone or that player is still in the game. If not, that
        // Aura, Equipment, or Fortification phases in unattached." P0's Holy Strength on P1's
        // blue 2/1 phases out alone, so the 2/1 loses its bonus. P1 then destroys the 2/1 with
        // Lightning Bolt; at P0's untap step Holy Strength phases in attached to nothing and is
        // put into the graveyard ([CR 704.5m]).
        //
        // [CR 702.26i]: https://yawgatog.com/resources/magic-rules/#R70226i
        // [CR 704.5m]: https://yawgatog.com/resources/magic-rules/#R7045m
        Scenario::new("an Aura that phased out alone phases in attached to nothing")
            .interaction(&[HOLY, BOLT])
            .battlefield(P0, &[Card::Plains; 4])
            .hand(P0, &[HOLY, PROTECTION])
            .battlefield(P1, &[Card::Mountain, Prop::Vanilla])
            .hand(P1, &[BOLT])
            .library(P0, &[Card::Plains; 3])
            .library(P1, &[Card::Plains; 3])
            .actions(&[
                ME.casts(HOLY),
                Resolve,
                THEM.pt(Prop::Vanilla, 3, 3),
                ME.casts(PROTECTION),
                Resolve,
                ME.has(Zone::Battlefield, HOLY, 0),
                THEM.pt(Prop::Vanilla, 2, 1),
                ME.passes(),
                THEM.casts(BOLT),
                THEM.targets(Prop::Vanilla),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
                advance_to(3, TurnStep::Upkeep),
                ME.has(Zone::Battlefield, HOLY, 0),
                ME.has(Zone::Graveyard, HOLY, 1),
            ]),
        // [CR 502.1]: "all phased-out permanents that the active player controlled when they
        // phased out phase in." P0's kicked Thieving Skydiver takes P1's Portable Hole; Teferi's
        // Protection phases the Hole out under P0's control. It stays out through P1's untap step
        // and phases in at P0's, still P0's, without its enters ability triggering
        // ([CR 702.26d]).
        //
        // [CR 502.1]: https://yawgatog.com/resources/magic-rules/#R5021
        // [CR 702.26d]: https://yawgatog.com/resources/magic-rules/#R70226d
        Scenario::new("a stolen artifact phases in at its thief's untap step")
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
            .hand(P0, &[SKYDIVER, PROTECTION])
            .battlefield(P1, &[Prop::Artifact])
            .library(P0, &[Card::Plains; 3])
            .library(P1, &[Card::Plains; 3])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: SKYDIVER,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.x(1),
                Resolve,
                Resolve,
                ME.controls(Prop::Artifact, 1),
                ME.casts(PROTECTION),
                Resolve,
                ME.controls(Prop::Artifact, 0),
                advance_to(2, TurnStep::Upkeep),
                ME.controls(Prop::Artifact, 0),
                THEM.controls(Prop::Artifact, 0),
                advance_to(3, TurnStep::Upkeep),
                ME.controls(Prop::Artifact, 1),
                stack(0),
            ]),
        Scenario::new("a phased-out Phantasmal Image is not sacrificed")
            .interaction(&[IMAGE])
            .battlefield(
                P0,
                &[
                    Card::Island,
                    Card::Island,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    SCIMITAR,
                ],
            )
            .hand(P0, &[IMAGE, PROTECTION])
            .battlefield(P1, &[Prop::Vanilla])
            .library(P0, &[Card::Plains; 3])
            .library(P1, &[Card::Plains; 3])
            .actions(&[
                ME.casts(IMAGE),
                Resolve,
                ME.picks_up_to(Prop::Vanilla),
                ME.controls(Prop::Vanilla, 1),
                ME.activates(SCIMITAR, 1), // Equip {1}, targeting the Image
                ME.casts(PROTECTION),
                Resolve,
                Resolve,
                Resolve,
                ME.has(Zone::Graveyard, IMAGE, 0),
                ME.controls(Prop::Vanilla, 0),
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                THEM.done_attacking(),
                advance_to(3, TurnStep::Upkeep),
                ME.controls(Prop::Vanilla, 1),
                ME.pt(Prop::Vanilla, 2, 1),
            ]),
        // [CR 702.26g]: "When a permanent phases out, any Auras, Equipment, or Fortifications
        // attached to that permanent phase out at the same time." Phased out, the 2/1, its
        // Equipment and the lands are on no battlefield; Teferi's Protection is exiled. At P0's
        // next untap step all of them phase in, the Equipment still attached (the ruling: "Each
        // Aura and Equipment that phases out attached to a permanent that's phasing out phases in
        // with that permanent and still attached to it.").
        //
        // [CR 702.26g]: https://yawgatog.com/resources/magic-rules/#R70226g
        Scenario::new("an equipped creature phases out and in with its Equipment")
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    SCIMITAR,
                    Prop::Vanilla,
                ],
            )
            .hand(P0, &[PROTECTION])
            .library(P0, &[Card::Plains; 3])
            .library(P1, &[Card::Plains; 3])
            .actions(&[
                ME.activates(SCIMITAR, 1), // Equip {1}
                Resolve,
                ME.pt(Prop::Vanilla, 3, 2),
                ME.casts(PROTECTION),
                Resolve,
                ME.has(Zone::Exile, PROTECTION, 1),
                ME.has(Zone::Battlefield, Prop::Vanilla, 0),
                ME.has(Zone::Battlefield, SCIMITAR, 0),
                ME.has(Zone::Battlefield, Card::Plains, 0),
                advance_to(3, TurnStep::Upkeep),
                ME.has(Zone::Battlefield, Prop::Vanilla, 1),
                ME.has(Zone::Battlefield, SCIMITAR, 1),
                ME.pt(Prop::Vanilla, 3, 2),
                ME.tapped(Card::Plains, 0),
            ]),
        // The ruling: "A permanent phasing out causes a spell or ability on the stack to have an
        // illegal target if it targets that permanent." Lightning Bolt's one target is illegal,
        // so it does nothing ([CR 608.2b]), and the 2/1 phases in at P0's untap step.
        //
        // [CR 608.2b]: https://yawgatog.com/resources/magic-rules/#R6082b
        in_their_turn("a creature phased out in response to Lightning Bolt survives").actions(&[
            THEM.casts(BOLT),
            THEM.targets(Prop::Vanilla),
            ME.casts(PROTECTION),
            Resolve,
            ME.has(Zone::Battlefield, Prop::Vanilla, 0),
            advance_to(2, TurnStep::BeginningOfCombat),
            THEM.has(Zone::Graveyard, BOLT, 1),
            THEM.passes(),
            THEM.done_attacking(),
            advance_to(3, TurnStep::Upkeep),
            ME.has(Zone::Battlefield, Prop::Vanilla, 1),
            ME.has(Zone::Graveyard, Prop::Vanilla, 0),
        ]),
        // [CR 702.16j]: a player with protection from everything "can't be targeted by spells or
        // abilities". Lightning Bolt may target only P1 and P1's own creature: P0 is protected,
        // and P0's creature is phased out.
        //
        // [CR 702.16j]: https://yawgatog.com/resources/magic-rules/#R70216j
        in_their_turn("Lightning Bolt can't target a player with protection from everything")
            .actions(&[
                THEM.passes(),
                ME.casts(PROTECTION),
                Resolve,
                THEM.casts(BOLT),
                THEM.deciding(DecisionKind::Target, 2),
                THEM.targets(Prop::Flier),
                Resolve,
                THEM.has(Zone::Graveyard, Prop::Flier, 1),
            ]),
        // [CR 702.15b]: lifelink gains life for the damage dealt. Damage that is prevented is
        // not dealt ([CR 615.1]), so P1's Griselbrand attacking the protected P0 gains P1
        // nothing, and P0's life total is unchanged.
        //
        // [CR 702.15b]: https://yawgatog.com/resources/magic-rules/#R70215b
        // [CR 615.1]: https://yawgatog.com/resources/magic-rules/#R6151
        Scenario::new("prevented combat damage from a lifelink attacker gains no life")
            .interaction(&[Card::Griselbrand])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Card::Plains, Card::Plains, Card::Plains])
            .hand(P0, &[PROTECTION])
            .battlefield(P1, &[Card::Griselbrand])
            .library(P0, &[Card::Plains; 3])
            .library(P1, &[Card::Plains; 3])
            .actions(&[
                THEM.passes(),
                ME.casts(PROTECTION),
                Resolve,
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                THEM.attacks_with(Card::Griselbrand),
                advance_to(2, TurnStep::EndOfCombat),
                ME.life(20),
                THEM.life(20),
            ]),
        // A token that phases out does not cease to exist ([CR 702.26b] only treats it as
        // nonexistent; [CR 111.7] ends a token's existence only when it is in a zone other than
        // the battlefield). It phases in at P0's next untap step. P0's Treasure comes from Beza,
        // who enters while P1 has more lands.
        //
        // [CR 111.7]: https://yawgatog.com/resources/magic-rules/#R1117
        Scenario::new("a token that phases out phases back in")
            .interaction(&[Card::BezaTheBoundingSpring, Card::Hullbreacher])
            .hand(P1, &[Card::Hullbreacher])
            .battlefield(P0, &[Card::Plains; 7])
            .battlefield(P1, &[Card::Plains; 8])
            .hand(P0, &[Card::BezaTheBoundingSpring, PROTECTION])
            .library(P0, &[Card::Plains; 3])
            .library(P1, &[Card::Plains; 3])
            .actions(&[
                ME.casts(Card::BezaTheBoundingSpring),
                Resolve,
                Resolve,
                ME.controls(crate::tokens::TREASURE_TOKEN, 1),
                ME.casts(PROTECTION),
                Resolve,
                ME.controls(crate::tokens::TREASURE_TOKEN, 0),
                advance_to(3, TurnStep::Upkeep),
                ME.controls(crate::tokens::TREASURE_TOKEN, 1),
            ]),
        // Phased out, P1's permanents stay out for the rest of P1's turn: P1, with a land still
        // to play, and P0, with Lightning Bolt to cast in P1's end step, both see P1's lands
        // and 2/1 phased out.
        Scenario::new("permanents stay phased out for the rest of the turn")
            .interaction(&[BOLT])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(
                P1,
                &[Card::Plains, Card::Plains, Card::Plains, Prop::Vanilla],
            )
            .hand(P1, &[PROTECTION, Card::Plains])
            .battlefield(P0, &[Card::Mountain, Prop::Flier])
            .hand(P0, &[BOLT])
            .library(P0, &[Card::Plains; 3])
            .library(P1, &[Card::Plains; 3])
            .actions(&[
                THEM.casts(PROTECTION),
                Resolve,
                THEM.has(Zone::Battlefield, Prop::Vanilla, 0),
                THEM.has(Zone::Battlefield, Card::Plains, 0),
                advance_to(2, TurnStep::End),
                THEM.has(Zone::Battlefield, Prop::Vanilla, 0),
                THEM.has(Zone::Exile, PROTECTION, 1),
            ]),
        // The ruling: "Creatures can still attack you while you have protection from everything,
        // although combat damage that they would deal to you will be prevented." P1's 3/1
        // attacks an empty board; P0's life total stays at 20.
        in_their_turn("an attack into an empty board deals no damage").actions(&[
            THEM.passes(),
            ME.casts(PROTECTION),
            Resolve,
            advance_to(2, TurnStep::BeginningOfCombat),
            THEM.passes(),
            THEM.attacks_with(Prop::Flier),
            advance_to(2, TurnStep::EndOfCombat),
            ME.life(20),
            THEM.life(20),
        ]),
        // The ruling: "Any creatures that phase in under your control as your next untap step
        // begins will be able to attack and pay a cost of {T} during that turn." The 2/1 that
        // phased in attacks for 2, and P0's life total can change again.
        in_their_turn("a creature that phased in attacks that turn").actions(&[
            THEM.passes(),
            ME.casts(PROTECTION),
            Resolve,
            advance_to(2, TurnStep::BeginningOfCombat),
            THEM.passes(),
            THEM.done_attacking(),
            advance_to(3, TurnStep::BeginningOfCombat),
            ME.passes(),
            THEM.passes(),
            ME.attacks_with(Prop::Vanilla),
            THEM.passes(),
            THEM.answers(DecisionKind::Blocker, Pick::Done),
            advance_to(3, TurnStep::EndOfCombat),
            THEM.life(18),
        ]),
        // The life-gain instruction has no effect. Beza's same trigger still makes Treasure.
        Scenario::new("Teferi's Protection stops Beza's life gain while other effects resolve")
            .interaction(&[Card::BezaTheBoundingSpring])
            .life(P0, 15)
            .hand(P0, &[Card::BezaTheBoundingSpring, PROTECTION])
            .battlefield(P0, &[Card::Plains; 7])
            .battlefield(P1, &[Card::Island])
            .actions(&[
                ME.casts(Card::BezaTheBoundingSpring),
                Resolve,
                ME.casts(PROTECTION),
                Resolve,
                Resolve,
                ME.life(15),
                ME.controls(crate::tokens::TREASURE_TOKEN, 1),
            ]),
        // Gaining protection in response makes the only target illegal, so even the life-gain
        // instruction for Tendrils' caster is skipped.
        Scenario::new("Teferi's Protection makes Tendrils of Agony lose its only target")
            .interaction(&[Card::TendrilsOfAgony])
            .at(2, P1, TurnStep::PrecombatMain)
            .hand(P0, &[PROTECTION])
            .battlefield(P0, &[Card::Plains; 3])
            .hand(P1, &[Card::TendrilsOfAgony])
            .battlefield(P1, &[Card::Swamp; 4])
            .actions(&[
                THEM.casts(Card::TendrilsOfAgony),
                THEM.targets_player(ME),
                ME.casts(PROTECTION),
                stack(0),
                ME.life(20),
                THEM.life(20),
                THEM.has(Zone::Graveyard, Card::TendrilsOfAgony, 1),
            ]),
        // A blue card is available, but P0 cannot pay the one life for Force of Will.
        Scenario::new("Teferi's Protection prevents paying Force of Will's alternative cost")
            .interaction(&[Card::ForceOfWill])
            .at(2, P1, TurnStep::PrecombatMain)
            .hand(P0, &[PROTECTION, Card::ForceOfWill, Card::Counterspell])
            .battlefield(P0, &[Card::Plains; 3])
            .hand(P1, &[Prop::Equipment])
            .battlefield(P1, &[Card::Plains])
            .actions(&[
                THEM.passes(),
                ME.casts(PROTECTION),
                Resolve,
                THEM.casts(Prop::Equipment),
                THEM.controls(Prop::Equipment, 1),
                ME.life(20),
                ME.has(Zone::Hand, Card::ForceOfWill, 1),
                ME.has(Zone::Hand, Card::Counterspell, 1),
            ]),
        // Phasing is not a zone change. Portable Hole keeps its exiled card while phased
        // out, and its enters ability does not trigger when it phases in.
        Scenario::new("Phasing Portable Hole keeps its exiled card in exile")
            .hand(P0, &[PROTECTION, Prop::Artifact])
            .battlefield(P0, &[Card::Plains; 4])
            .battlefield(P1, &[Prop::Vanilla])
            .actions(&[
                ME.casts(Prop::Artifact),
                Resolve,
                Resolve,
                THEM.has(Zone::Exile, Prop::Vanilla, 1),
                ME.casts(PROTECTION),
                Resolve,
                ME.controls(Prop::Artifact, 0),
                THEM.has(Zone::Exile, Prop::Vanilla, 1),
                advance_to(3, TurnStep::Upkeep),
                ME.controls(Prop::Artifact, 1),
                THEM.has(Zone::Exile, Prop::Vanilla, 1),
                stack(0),
            ]),
        // The creature retains its counters through both halves of phasing.
        Scenario::new("A creature keeps its counters through phasing")
            .hand(P0, &[PROTECTION])
            .battlefield(P0, &[Card::Plains; 3])
            .permanents(
                P0,
                &[Permanent::new(Prop::Vanilla).counters(CounterKind::PlusOne, 2)],
            )
            .actions(&[
                ME.casts(PROTECTION),
                Resolve,
                ME.controls(Prop::Vanilla, 0),
                advance_to(3, TurnStep::Upkeep),
                ME.controls(Prop::Vanilla, 1),
                ME.counters(Prop::Vanilla, CounterKind::PlusOne, 2),
            ]),
        // Painter's Servant stops affecting objects during phasing and remembers its
        // chosen color when it phases in.
        Scenario::new("Painter's Servant remembers its chosen color through phasing")
            .interaction(&[Card::PaintersServant])
            .hand(P0, &[PROTECTION, Card::PaintersServant])
            .battlefield(P0, &[Card::Plains; 5])
            .battlefield(P1, &[Card::Island])
            .actions(&[
                ME.casts(Card::PaintersServant),
                Resolve,
                ME.chooses_color(Colors::RED),
                THEM.colored(Zone::Battlefield, Card::Island, Colors::RED),
                ME.casts(PROTECTION),
                Resolve,
                THEM.colored(Zone::Battlefield, Card::Island, Colors::NONE),
                advance_to(3, TurnStep::Upkeep),
                THEM.colored(Zone::Battlefield, Card::Island, Colors::RED),
            ]),
        // The caster still draws both cards from Night's Whisper while its life loss is blocked.
        Scenario::new("Teferi's Protection stops Night's Whisper's life loss but allows the draw")
            .interaction(&[Card::NightsWhisper])
            .hand(P0, &[Card::NightsWhisper, PROTECTION])
            .battlefield(
                P0,
                &[
                    Card::Swamp,
                    Card::Swamp,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .library(P0, &[Card::Island, Card::Mountain])
            .actions(&[
                ME.casts(Card::NightsWhisper),
                ME.casts(PROTECTION),
                Resolve,
                Resolve,
                ME.has(Zone::Hand, Card::Island, 1),
                ME.has(Zone::Hand, Card::Mountain, 1),
                ME.life(20),
            ]),
        // The phased-out creature is an illegal target, but the other target still
        // exists, so Aura Finesse still draws a card for its caster.
        Scenario::new("Aura Finesse still draws when only its creature target phases out")
            .interaction(&[Card::AuraFinesse, HOLY])
            .at(2, P1, TurnStep::PrecombatMain)
            .hand(P0, &[PROTECTION])
            .battlefield(
                P0,
                &[Card::Plains, Card::Plains, Card::Plains, Prop::Vanilla],
            )
            .hand(P1, &[HOLY, Card::AuraFinesse])
            .battlefield(P1, &[Card::Plains, Card::Island, Prop::Flier])
            .library(P1, &[Card::Mountain])
            .actions(&[
                THEM.casts(HOLY),
                THEM.targets(Prop::Flier),
                Resolve,
                THEM.casts(Card::AuraFinesse),
                THEM.targets(Prop::Vanilla),
                ME.casts(PROTECTION),
                Resolve,
                ME.controls(Prop::Vanilla, 0),
                THEM.pt(Prop::Flier, 4, 3),
                THEM.has(Zone::Hand, Card::Mountain, 1),
            ]),
        // Protection has no bearing on a discard instruction that doesn't target its player.
        Scenario::new("Teferi's Protection allows discarding to Jace's untargeted ability")
            .interaction(&[Card::JaceVrynsProdigy])
            .hand(P0, &[PROTECTION, Card::Mountain])
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::JaceVrynsProdigy,
                ],
            )
            .library(P0, &[Card::Island])
            .actions(&[
                ME.activates(Card::JaceVrynsProdigy, 0),
                ME.casts(PROTECTION),
                Resolve,
                Resolve,
                ME.picks(Card::Mountain),
                ME.has(Zone::Graveyard, Card::Mountain, 1),
                ME.has(Zone::Hand, Card::Island, 1),
            ]),
    ];
}
