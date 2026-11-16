// Generated from the card store by `just card header`: notes go below the end marker.
//! Gemrazer {3}{G}
//! Creature — Beast, 4/4
//!
//! Mutate {1}{G}{G} (If you cast this spell for its mutate cost, put it over or under target
//! non-Human creature you own. They mutate into the creature on top plus all abilities from under
//! it.)
//! Reach, trample
//! Whenever this creature mutates, destroy target artifact or enchantment an opponent controls.
// End of the generated card header.

use crate::prelude::*;

/// Mutate is an alternative cost ([CR 702.140a]); the spell ability at index 1 is what only
/// the mutating creature spell has: its target and its merge ([`Effect::Mutate`]). Cast for
/// its mana cost, it is a plain creature spell with no target. Its triggered ability
/// triggers for the merged permanent whether Gemrazer went over or under (CR 702.140d).
///
/// [CR 702.140a]: https://yawgatog.com/resources/magic-rules/#R702140a
pub static CARD: CardDef = Printed::Gemrazer
    .keywords(Keywords::REACH.union(Keywords::TRAMPLE))
    .abilities(&[
        Ability::triggered(&Trigger::MUTATES)
            .target(&TargetSpec::permanent(Filter::All(&[
                Filter::Types(Types::ARTIFACT.union(Types::ENCHANTMENT)),
                OPPONENT_CONTROLS,
            ])))
            .effect(&destroy(target())),
        Ability::spell()
            .target(&TargetSpec::permanent(Filter::All(&[
                Filter::Types(Types::CREATURE),
                Filter::Not(&Filter::Subtype(Subtype::Human)),
                Filter::Owned(&Players::You),
            ])))
            .effect(&Effect::Mutate(target())),
    ])
    .casting(&[CastingOption::mutate(mana("{1}{G}{G}"), 1)])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;
    use crate::tokens::TREASURE_TOKEN;

    /// The cast for the mutate cost, its first option.
    const MUTATE: Action = Action::Cast {
        player: P0,
        card: Card::Gemrazer,
        face: FaceIx::FRONT,
        costs: Castings::only(0),
    };

    pub const SCENARIOS: &[Scenario] = &[
        // Under, the merged creature is Blood Artist, a 0/1 Vampire, with Gemrazer's abilities:
        // its "whenever this creature mutates" triggers and destroys P1's Scimitar. Gemrazer's
        // card is on the battlefield as part of it, and no creature entered.
        Scenario::new("Gemrazer under a creature: the creature's values, Gemrazer's abilities")
            .interaction(&[Card::BloodArtist])
            .battlefield(
                P0,
                &[Card::Forest, Card::Forest, Card::Forest, Card::BloodArtist],
            )
            .hand(P0, &[Card::Gemrazer])
            .battlefield(P1, &[Card::LeoninScimitar])
            .library(P1, &[Card::Island])
            .actions(&[
                MUTATE,
                Resolve,
                ME.no(),
                Resolve,
                THEM.controls(Card::LeoninScimitar, 0),
                THEM.has(Zone::Graveyard, Card::LeoninScimitar, 1),
                ME.controls(Card::BloodArtist, 1),
                ME.pt(Card::BloodArtist, 0, 1),
                ME.controls(Card::Gemrazer, 0),
                ME.has_cards(Zone::Hand, 0),
                ME.has_cards(Zone::Graveyard, 0),
            ]),
        // Over, it is a 4/4 Gemrazer, and its trigger is Gemrazer's own.
        Scenario::new("Gemrazer over a creature: a 4/4 Gemrazer that mutated")
            .interaction(&[Card::BloodArtist])
            .battlefield(
                P0,
                &[Card::Forest, Card::Forest, Card::Forest, Card::BloodArtist],
            )
            .hand(P0, &[Card::Gemrazer])
            .battlefield(P1, &[Card::LeoninScimitar])
            .library(P1, &[Card::Island])
            .actions(&[
                MUTATE,
                Resolve,
                ME.yes(),
                Resolve,
                THEM.controls(Card::LeoninScimitar, 0),
                ME.controls(Card::Gemrazer, 1),
                ME.pt(Card::Gemrazer, 4, 4),
                ME.controls(Card::BloodArtist, 0),
            ]),
        // Death Ward sets up a regeneration shield before the mutate trigger resolves.
        Scenario::new("Gemrazer's mutate trigger allows its target to regenerate")
            .battlefield(
                P0,
                &[Card::Forest, Card::Forest, Card::Forest, Prop::Vanilla],
            )
            .hand(P0, &[Card::Gemrazer])
            .battlefield(P1, &[Card::Plains, Card::Plains, Card::Ornithopter])
            .hand(P1, &[Prop::Regeneration, Prop::Regeneration])
            .library(P1, &[Card::Island])
            .actions(&[
                MUTATE,
                Resolve,
                ME.yes(),
                THEM.casts(Prop::Regeneration),
                THEM.targets(Card::Ornithopter),
                Resolve,
                Resolve,
                THEM.controls(Card::Ornithopter, 1),
                THEM.tapped(Card::Ornithopter, 1),
            ]),
        // The merged creature dies: one creature died, so Blood Artist's "whenever this creature
        // or another creature dies", from under Gemrazer, triggers once (CR 731.3), and both
        // cards go to P0's graveyard.
        Scenario::new("Gemrazer's merged creature dies: both cards go, a dies trigger once")
            .interaction(&[Card::BloodArtist, Card::GetLost])
            .battlefield(
                P0,
                &[Card::Forest, Card::Forest, Card::Forest, Card::BloodArtist],
            )
            .hand(P0, &[Card::Gemrazer])
            .battlefield(P1, &[Card::Plains, Card::Plains, Card::LeoninScimitar])
            .hand(P1, &[Card::GetLost])
            .library(P1, &[Card::Island])
            .actions(&[
                MUTATE,
                Resolve,
                ME.yes(),
                Resolve,
                ME.passes(),
                THEM.casts(Card::GetLost),
                Resolve,
                ME.targets_player(THEM),
                Resolve,
                ME.controls(Card::Gemrazer, 0),
                ME.has(Zone::Graveyard, Card::Gemrazer, 1),
                ME.has(Zone::Graveyard, Card::BloodArtist, 1),
                THEM.life(19),
                ME.life(21),
            ]),
        // Bounced, the merged creature's cards both go to P0's hand.
        Scenario::new("Gemrazer's merged creature bounced: both cards to hand")
            .interaction(&[Card::BloodArtist])
            .battlefield(
                P0,
                &[Card::Forest, Card::Forest, Card::Forest, Card::BloodArtist],
            )
            .hand(P0, &[Card::Gemrazer])
            .battlefield(
                P1,
                &[
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::LeoninScimitar,
                ],
            )
            .hand(P1, &[Card::SinkIntoStupor])
            .library(P1, &[Card::Island])
            .actions(&[
                MUTATE,
                Resolve,
                ME.no(),
                Resolve,
                ME.passes(),
                THEM.casts(Card::SinkIntoStupor),
                ME.controls(Card::BloodArtist, 0),
                ME.has(Zone::Hand, Card::Gemrazer, 1),
                ME.has(Zone::Hand, Card::BloodArtist, 1),
            ]),
        // The target dies in response: the spell is no longer a mutating creature spell and
        // enters as a 4/4 Gemrazer (CR 702.140b), which did not mutate, so no trigger.
        Scenario::new("Gemrazer's target gone: it enters as a creature and does not mutate")
            .interaction(&[Card::BloodArtist, Card::GetLost])
            .battlefield(
                P0,
                &[Card::Forest, Card::Forest, Card::Forest, Card::BloodArtist],
            )
            .hand(P0, &[Card::Gemrazer])
            .battlefield(P1, &[Card::Plains, Card::Plains, Card::LeoninScimitar])
            .hand(P1, &[Card::GetLost])
            .library(P1, &[Card::Island])
            .actions(&[
                MUTATE,
                THEM.casts(Card::GetLost),
                Resolve,
                ME.targets_player(THEM),
                Resolve,
                Resolve,
                ME.controls(Card::Gemrazer, 1),
                ME.pt(Card::Gemrazer, 4, 4),
                THEM.controls(Card::LeoninScimitar, 1),
                stack(0),
            ]),
        // Glasspool Mimic copies the merged creature: its copiable values are Gemrazer's with
        // every ability of Blood Artist (CR 731.2a, CR 707.2), so the copy has Blood Artist's
        // ability too. When a third creature dies, the merged creature and the copy both trigger.
        Scenario::new(
            "Glasspool Mimic copying Gemrazer's merged creature has both cards' abilities",
        )
        .interaction(&[Card::BloodArtist, Card::GetLost, Card::GlasspoolMimic])
        .battlefield(
            P0,
            &[
                Card::Forest,
                Card::Forest,
                Card::Forest,
                Card::Island,
                Card::Island,
                Card::Island,
                Card::BloodArtist,
            ],
        )
        .hand(P0, &[Card::Gemrazer, Card::GlasspoolMimic])
        .battlefield(
            P1,
            &[
                Card::Plains,
                Card::Plains,
                Card::LeoninScimitar,
                Prop::Flier,
            ],
        )
        .hand(P1, &[Card::GetLost])
        .library(P1, &[Card::Island])
        .actions(&[
            MUTATE,
            Resolve,
            ME.yes(),
            Resolve,
            ME.casts(Card::GlasspoolMimic),
            Resolve,
            ME.picks_up_to(Card::Gemrazer),
            ME.controls(Card::Gemrazer, 2),
            THEM.casts(Card::GetLost),
            THEM.targets(Prop::Flier),
            ME.chooses(DecisionKind::TriggerOrder, Card::Gemrazer),
            ME.targets_player(THEM),
            ME.targets_player(THEM),
            Resolve,
            Resolve,
            ME.controls(Card::Gemrazer, 2),
            THEM.controls(Prop::Flier, 0),
            THEM.life(18),
            ME.life(22),
        ]),
        // Over Drudge Skeletons, the merged creature is a Gemrazer with the Skeletons' "{B}:
        // Regenerate this creature" (CR 702.140e). Activated in response to Get Lost, it
        // regenerates: the merged creature stays, tapped, and both cards stay with it.
        Scenario::new("Gemrazer over Drudge Skeletons activates the Skeletons' ability")
            .interaction(&[Card::DrudgeSkeletons, Card::GetLost])
            .battlefield(
                P0,
                &[
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                    Card::Swamp,
                    Card::DrudgeSkeletons,
                    Card::MerfolkOfThePearlTrident,
                ],
            )
            .hand(P0, &[Card::Gemrazer])
            .battlefield(P1, &[Card::Plains, Card::Plains, Card::LeoninScimitar])
            .hand(P1, &[Card::GetLost])
            .library(P1, &[Card::Island])
            .actions(&[
                MUTATE,
                ME.targets(Card::DrudgeSkeletons),
                Resolve,
                ME.yes(),
                Resolve,
                ME.passes(),
                THEM.casts(Card::GetLost),
                THEM.targets(Card::Gemrazer),
                ME.activates_merged(Card::Gemrazer, 0, 0),
                Resolve,
                Resolve,
                ME.controls(Card::Gemrazer, 1),
                ME.tapped(Card::Gemrazer, 1),
                ME.has_cards(Zone::Graveyard, 0),
            ]),
        // Over Gold Myr, the merged creature taps for the Myr's {W} (CR 702.140e). It has been
        // on the battlefield since the turn began, as the Myr was: mutating moved nothing onto
        // the battlefield (CR 731.2c). Its {W} and the fourth Forest cast Get Lost.
        Scenario::new("Gemrazer over Gold Myr taps for the Myr's mana")
            .interaction(&[Card::GoldMyr, Card::GetLost])
            .battlefield(
                P0,
                &[
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                    Card::GoldMyr,
                ],
            )
            .hand(P0, &[Card::Gemrazer, Card::GetLost])
            .battlefield(P1, &[Card::LeoninScimitar, Prop::Vanilla])
            .library(P1, &[Card::Island])
            .actions(&[
                MUTATE,
                Resolve,
                ME.yes(),
                Resolve,
                ME.casts(Card::GetLost),
                ME.targets(Prop::Vanilla),
                Resolve,
                THEM.controls(Prop::Vanilla, 0),
                ME.tapped(Card::Gemrazer, 1),
            ]),
        // Over Lord of Atlantis, the merged creature is a Gemrazer whose static ability, from
        // under it, gives other Merfolk +1/+1 and islandwalk (CR 702.140e, CR 613.4c).
        Scenario::new("Gemrazer over Lord of Atlantis: the Lord's anthem applies")
            .interaction(&[Card::LordOfAtlantis])
            .battlefield(
                P0,
                &[
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                    Card::LordOfAtlantis,
                    Card::MerfolkOfThePearlTrident,
                ],
            )
            .hand(P0, &[Card::Gemrazer])
            .battlefield(P1, &[Card::LeoninScimitar])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.pt(Card::MerfolkOfThePearlTrident, 2, 2),
                MUTATE,
                ME.targets(Card::LordOfAtlantis),
                Resolve,
                ME.yes(),
                Resolve,
                ME.controls(Card::LordOfAtlantis, 0),
                ME.pt(Card::Gemrazer, 4, 4),
                ME.pt(Card::MerfolkOfThePearlTrident, 2, 2),
            ]),
        // Over Platinum Emperion, the merged creature is a Gemrazer with the Emperion's "Your
        // life total can't change", a rule change from under it (CR 702.140e, CR 613.11):
        // Lightning Bolt deals P0 3 damage and P0 loses no life.
        Scenario::new("Gemrazer over Platinum Emperion: P0's life total can't change")
            .interaction(&[Card::PlatinumEmperion, Card::LightningBolt])
            .battlefield(
                P0,
                &[
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                    Card::PlatinumEmperion,
                ],
            )
            .hand(P0, &[Card::Gemrazer])
            .battlefield(P1, &[Card::Mountain, Card::LeoninScimitar])
            .hand(P1, &[Card::LightningBolt])
            .library(P1, &[Card::Island])
            .actions(&[
                MUTATE,
                Resolve,
                ME.yes(),
                Resolve,
                ME.controls(Card::PlatinumEmperion, 0),
                ME.passes(),
                THEM.casts(Card::LightningBolt),
                THEM.targets_player(ME),
                THEM.has(Zone::Graveyard, Card::LightningBolt, 1),
                ME.life(20),
            ]),
        // Over Hullbreacher, the merged creature is a Gemrazer with the Hullbreacher's
        // replacement of an opponent's draws, from under it (CR 702.140e, CR 614.1a): the draw
        // of P1's Twisted Image makes P0 a Treasure instead. The Forest left in hand keeps
        // P0's main phase a decision once the Twisted Image has resolved.
        Scenario::new("Gemrazer over Hullbreacher: P1's draw is a Treasure for P0")
            .interaction(&[Card::Hullbreacher, Card::TwistedImage])
            .battlefield(
                P0,
                &[Card::Forest, Card::Forest, Card::Forest, Card::Hullbreacher],
            )
            .hand(P0, &[Card::Gemrazer, Card::Forest])
            .battlefield(P1, &[Card::Island, Card::LeoninScimitar])
            .hand(P1, &[Card::TwistedImage])
            .library(P1, &[Card::Island])
            .actions(&[
                MUTATE,
                Resolve,
                ME.yes(),
                Resolve,
                ME.controls(Card::Hullbreacher, 0),
                ME.passes(),
                THEM.casts(Card::TwistedImage),
                ME.controls(TREASURE_TOKEN, 1),
                THEM.has_cards(Zone::Hand, 0),
                THEM.library_top(&[Card::Island]),
            ]),
        // Over Bloodghast, the merged creature is a Gemrazer with the Bloodghast's "This
        // creature can't block", a rule change of its own from under it (CR 702.140e,
        // CR 509.1a): P0 is never asked to block, and P1's 2/1 deals its damage.
        Scenario::new("Gemrazer over Bloodghast: the merged creature can't block")
            .interaction(&[Card::Bloodghast])
            .battlefield(
                P0,
                &[Card::Forest, Card::Forest, Card::Forest, Card::Bloodghast],
            )
            .hand(P0, &[Card::Gemrazer])
            .battlefield(P1, &[Card::LeoninScimitar, Prop::Vanilla])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                MUTATE,
                Resolve,
                ME.yes(),
                Resolve,
                ME.controls(Card::Bloodghast, 0),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.done_attacking(),
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                THEM.attacks_with(Prop::Vanilla),
                advance_to(2, TurnStep::EndOfCombat),
                ME.life(18),
            ]),
        // Over a creature manifested by Abhorrent Oculus, the merged creature is a face-up 4/4
        // Gemrazer that both players see, and its card under it stays face down, known only to
        // P0 (CR 731.2e).
        Scenario::new("Gemrazer over a face-down creature: a face-up Gemrazer, its card hidden")
            .interaction(&[Card::AbhorrentOculus])
            .at(1, P0, TurnStep::PostcombatMain)
            .battlefield(
                P0,
                &[
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                    Card::AbhorrentOculus,
                ],
            )
            .hand(P0, &[Card::Gemrazer])
            .library(
                P0,
                &[
                    Prop::Vanilla,
                    Card::Counterspell,
                    Card::Island,
                    Card::Island,
                ],
            )
            .battlefield(P1, &[Card::LeoninScimitar])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                advance_to(2, TurnStep::Upkeep),
                Resolve,
                ME.picks(Prop::Vanilla),
                ME.face_down(Prop::Vanilla, 1),
                advance_to(3, TurnStep::PrecombatMain),
                MUTATE,
                ME.targets(Prop::Vanilla),
                Resolve,
                ME.yes(),
                Resolve,
                THEM.controls(Card::LeoninScimitar, 0),
                ME.controls(Card::Gemrazer, 1),
                ME.face_down(Card::Gemrazer, 0),
                ME.pt(Card::Gemrazer, 4, 4),
            ]),
        // CR 702.140a: a mutating creature spell targets a non-Human creature its caster owns. The
        // only creature P0 has is a Human (Hired Blade), so with the three Forests that pay for
        // mutate and not the four that pay the mana cost, the cast is not offered.
        Scenario::new("Gemrazer can't mutate onto a Human")
            .battlefield(
                P0,
                &[
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                    Prop::FlashCreature,
                ],
            )
            .hand(P0, &[Card::Gemrazer])
            .library(P0, &[Card::Island])
            // With no cast offered, the only decision before combat is whether to attack with the
            // Hired Blade, so the game passes priority on its own.
            .actions(&[ME.deciding(DecisionKind::Attacker, 2)]),
        // CR 702.140a: "targets a non-Human creature with the same owner as this spell". A creature P1 owns is not a legal target even
        // under P0's control, and a creature P1 controls and owns certainly isn't.
        Scenario::new("Gemrazer can't mutate onto an opponent's creature")
            .battlefield(P0, &[Card::Forest, Card::Forest, Card::Forest])
            .hand(P0, &[Card::Gemrazer])
            .battlefield(P1, &[Prop::Vanilla])
            .library(P0, &[Card::Island])
            // With no cast offered, P0 has nothing to do in the turn; the game goes on to P1's
            // upkeep, before P1 draws.
            .actions(&[advance_to(2, TurnStep::Upkeep)]),
        // CR 730.2 / CR 702.140b: the merged permanent is the same object as the target, so
        // +1/+1 counters and attached Equipment stay on it: over a 2/1 with two +1/+1 counters
        // wearing a +1/+1 Equipment, a 4/4 Gemrazer is 7/7.
        Scenario::new("Gemrazer over a creature keeps its counters and Equipment")
            .permanents(
                P0,
                &[
                    Permanent::new(Prop::Vanilla).counters(CounterKind::PlusOne, 2),
                    Permanent::new(Prop::Equipment),
                ],
            )
            .battlefield(
                P0,
                &[Card::Forest, Card::Forest, Card::Forest, Card::Forest],
            )
            .hand(P0, &[Card::Gemrazer])
            .battlefield(P1, &[Card::LeoninScimitar])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(Prop::Equipment, 1),
                Resolve,
                ME.pt(Prop::Vanilla, 5, 4),
                MUTATE,
                Resolve,
                ME.yes(),
                Resolve,
                ME.pt(Card::Gemrazer, 7, 7),
                ME.counters(Card::Gemrazer, CounterKind::PlusOne, 2),
            ]),
        // CR 702.140 / CR 302.6: mutating moves nothing onto the battlefield, so a summoning
        // sick Gold Myr stays sick as the merged creature and can't pay {T} for its {W}; the
        // fourth Forest alone can't cast Get Lost ({1}{W}).
        Scenario::new("Gemrazer over a summoning-sick creature is still sick")
            .interaction(&[Card::GoldMyr, Card::GetLost])
            .permanents(P0, &[Permanent::new(Card::GoldMyr).sick()])
            .battlefield(
                P0,
                &[Card::Forest, Card::Forest, Card::Forest, Card::Forest],
            )
            .hand(P0, &[Card::Gemrazer, Card::GetLost])
            .battlefield(P1, &[Card::LeoninScimitar, Prop::Vanilla])
            .library(P1, &[Card::Island])
            .actions(&[
                MUTATE,
                Resolve,
                ME.yes(),
                Resolve,
                ME.deciding(DecisionKind::Priority, 1),
            ]),
    ];
}
