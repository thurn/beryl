//! [`Capability`]: what a game's cards may make the stores keep.
//!
//! Implements: none. Records which game mechanics the current card pool can use.

use beryl_card::definition::Shape;
use beryl_card::{
    Ability, AbilityKind, Amount, Behavior, CardDef, Condition, CostAction, CostPart, Designation,
    Draws, Effect, FACE_DOWN, Face, Filter, Keywords, Modification, Node, Objects, Replaced,
    Replacement, Rewrite, Subtype, Words, Zone,
};

use crate::event::Listens;
use crate::static_ability::Inputs;
use crate::subtypes::SubtypeNumbers;

/// Something a game's cards may make the stores keep (architecture §6.4, the crate's
/// contract, "Stores"), found once per game from every face of every card it can contain.
/// What no card of a game can need is not kept in it and costs it one AND.
///
/// The kinds of continuous effect have no bit here: the Effects store knows exactly which
/// are live ([`crate::effects`]), which a table could only bound, since a Rust body is
/// opaque to it. These are what no live entry can say: whether to keep a count nothing has
/// yet read. Whether a static ability changes a rule for players is no capability of the
/// game either: it is a bit of each card's replacement mask ([`crate::event::Replaces::PLAYER_RULE_CHANGE`]),
/// which says it of the permanents on the battlefield now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    /// A card reads whether a permanent entered the battlefield this turn, so that is
    /// tracked.
    EnteredThisTurn,
    /// A card has a loyalty ability, so whether one of a permanent's was activated this turn is
    /// tracked (see [CR 606.3]): "A player may activate a loyalty ability of a permanent
    /// they control any time they have priority and the stack is empty during a main phase of their
    /// turn, but only if no player has previously activated a loyalty ability of that permanent
    /// that turn."
    ///
    /// [CR 606.3]: https://yawgatog.com/resources/magic-rules/#R6063
    LoyaltyActivated,
    /// A card reads how many spells a player cast this turn
    /// ([`Amount::SpellsCast`]), so that is counted for each player.
    SpellsCast,
    /// A card reads how many cards a player drew this turn ([`Amount::CardsDrawn`]), so that
    /// is counted for each player.
    CardsDrawn,
    /// A card asks whether a player has drawn the first card of their draw step this turn
    /// ([`Condition::DrewInDrawStep`]), or replaces every draw but that one
    /// ([`Draws::ExceptFirstInDrawStep`]), so the cards each player draws in their own draw
    /// step are counted.
    DrewInDrawStep,
    /// A card grants another permanent an ability, so the abilities an object has
    /// ([`crate::State::abilities`]) include those its layer results say it was granted.
    Grants,
    /// A card grants another permanent a static ability with a continuous effect, so the
    /// layers may find an ability in layer 6 whose effect belongs to an earlier layer, and
    /// run once more with it present (`beryl_rules`' `layers`).
    GrantsLayerEffect,
    /// A card's static ability reads as the layers apply something no layer writes
    /// ([`Inputs`]), so the recompute finds what the permanents on the battlefield read.
    LayerInputs,
    /// A card has dredge ([`beryl_card::Replacement::dredge`]), a replacement of draws that
    /// works from a graveyard, so a draw looks there for one.
    Dredge,
    /// A card chooses a value as it enters ([`beryl_card::Replacement::choose_as_enters`]),
    /// may enter as a copy of a permanent it chooses
    /// ([`beryl_card::Replacement::enter_as_copy`]), or is an Aura, which chooses what it
    /// enchants as it enters other than as a resolving Aura spell (see CR 303.4f): a choice that
    /// may come up wherever a permanent enters, so every resolution runs where it can pause for
    /// it (`beryl_rules`' `resolve`).
    ChoosesAsEnters,
    /// A card's static ability affects objects in every zone
    /// ([`beryl_card::Objects::Everywhere`]), so the recompute lists such effects for the
    /// reads of characteristics off the battlefield ([`crate::query::zone_characteristics`]).
    ReachesElsewhere,
    /// A card has a mana ability whose mana is restricted ([`beryl_card::Ability::spend`]),
    /// so pools may hold mana that remembers ([`crate::mana_memory`]) and the mana solver
    /// looks for it.
    RemembersMana,
    /// A card has a characteristic-defining ability that defines its power or toughness
    /// ([`beryl_card::Modification::SetPower`], [`beryl_card::Modification::SetPt`]), which
    /// works in every zone, so a read of the power of an object off the battlefield looks for
    /// one ([`crate::query::defined_power`]).
    DefinesPt,
    /// A card has delve ([`beryl_card::Keywords::DELVE`]), so paying a spell's mana looks
    /// for cards in its caster's graveyard to pay generic mana with, and whether a spell is
    /// castable counts them.
    Delve,
    /// A card's copy exception gives a triggered ability ([`beryl_card::CopyChange::Has`]),
    /// which a permanent whose copiable values carry the exception has on no face of its
    /// definition, so trigger matching looks at the exceptions of copies (`beryl_rules`'
    /// `trigger`).
    GivesTriggers,
    /// An ability copies its spell ([`Effect::CopySpell`] of [`Objects::This`]) or reads its
    /// mana value from the stack. A spell that leaves while such an ability waits or is on
    /// the stack keeps the information the ability uses
    /// ([`crate::last_known::DepartedSpell`]).
    ReadsDepartedSpell,
    /// A card has mutate ([`beryl_card::CastingOption::Mutate`]), so a permanent may be made
    /// of several cards, and has the abilities of the faces of the cards under its top one
    /// ([`crate::query::Copiable::merged_faces`]), whose triggered abilities trigger matching
    /// looks for apart (`beryl_rules`' `trigger`).
    Merges,
    /// A card has a mana ability activated only as an instant ([`crate::table::is_instant_mana`]),
    /// so a priority decision looks for one to offer on its own.
    InstantMana,
    /// A card has an activated ability that works in a graveyard or in exile
    /// ([`beryl_card::Ability::in_zones`]), so priority decisions also look in those zones.
    ActivatesFromGraveyardOrExile,
    /// A card can change the sequence of steps or phases in a turn.
    TurnStructure,
    /// A mana ability can ask a question while a spell's costs are paid. Resolutions run
    /// on a copy so that such a question can pause from inside a payment.
    ManaAbilityChoices,
    /// A card can be cast face down for morph.
    Morph,
    /// The game may have a Saga or an ability with a chapter symbol.
    Saga,
    /// An Aura may be attached to a player.
    PlayerAttachments,
    /// An Equipment or a player attachment can become unattached.
    AttachmentStateChecks,
    /// A card has two halves with combined characteristics off the stack.
    Split,
    /// An ability depends on its permanent's Class level or solved designation.
    DesignatedAbilities,
    /// A spell can require sacrificing a permanent as part of its casting cost.
    SacrificeToCast,
    /// A card can be cast from its owner's library while they search it.
    CastWhileSearching,
    /// A cast trigger reads the card types of the spell that caused it to trigger.
    ReadsDepartedTriggeringSpell,
    /// A card has banding or "bands with other", so combat can declare attacking bands and
    /// change who assigns damage.
    Banding,
    /// A permanent can enter prepared and make a spell copy in exile.
    Preparation,
    /// A card reads whether a permanent was declared as an attacker this turn.
    AttackedThisTurn,
    /// A card asks whether a permanent was a spell its controller cast
    /// ([`Condition::Cast`]), so a permanent that was remembers it
    /// ([`crate::Memo::Cast`]).
    RemembersCast,
    /// A card's target filter compares with the power of its source
    /// ([`beryl_card::Filter::ManaValueAtMostPower`]), so choosing and checking targets reads
    /// that power.
    ReadsSourcePower,
    /// An activated ability's effect reads the mana spent on its X
    /// ([`Effect::MayCast`] with `within_x`), so paying an ability's cost records it on the
    /// ability's stack entry ([`crate::state::SpentMana`]).
    SpentOnX,
    /// A card has a companion ability ([`beryl_card::Ability::companion`]), so the game asks
    /// before the mulligans whether a player reveals a companion, and offers a revealed
    /// companion's special action at priority.
    Companion,
    /// A card lets its controller cast a spell from their graveyard once each turn
    /// ([`beryl_card::Ability::cast_from_graveyard_each_turn`]), so priority decisions look in
    /// graveyards for spells it permits and the permanents remember using it.
    CastsFromGraveyard,
}

// The game keeps its capabilities in a `u64`.
const _: () = assert!((Capability::CastsFromGraveyard as u32) < u64::BITS);

impl Capability {
    /// The capabilities of every face and subtype the game can contain.
    #[inline]
    pub(super) fn of_game(defs: &[&CardDef], subtypes: &SubtypeNumbers) -> u64 {
        let mut capabilities = defs
            .iter()
            .flat_map(|d| d.faces)
            .fold(Self::of(&FACE_DOWN), |c, f| c | Self::of(f));
        if subtypes.has(Subtype::Saga).possible() {
            capabilities |= Self::Saga.bit();
        }
        if subtypes.has(Subtype::Equipment).possible()
            || capabilities & Self::PlayerAttachments.bit() != 0
        {
            capabilities |= Self::AttachmentStateChecks.bit();
        }
        if defs
            .iter()
            .any(|def| matches!(def.shape, Shape::Split { .. }))
        {
            capabilities |= Self::Split.bit();
        }
        if defs.iter().any(|def| def.shape == Shape::Prepared) {
            capabilities |= Self::Preparation.bit();
        }
        capabilities
    }

    /// The capabilities a face gives a game that can contain it.
    fn of(face: &Face) -> u64 {
        let mut found = casting_capabilities(face) | keyword_capabilities(face.keywords);
        if Inputs::of(face) != Inputs::NONE {
            found |= Capability::LayerInputs.bit();
        }
        if !face.grants.is_empty() {
            found |= Capability::Grants.bit();
        }
        let gives_triggers = (0..face.grants.len()).any(|i| {
            face.grants[i].trigger().is_some() && crate::listener::given_by_exception(face, i)
        });
        if gives_triggers {
            found |= Capability::GivesTriggers.bit();
        }
        if face.subtypes.read(&Words::NONE).contains(Subtype::Aura) {
            found |= Capability::ChoosesAsEnters.bit();
            if face.abilities.iter().chain(face.grants).any(|ability| {
                ability.kind() == AbilityKind::Spell
                    && ability.targets().iter().any(|target| {
                        matches!(
                            target.kind,
                            beryl_card::TargetKind::Player
                                | beryl_card::TargetKind::PermanentOrPlayer
                        )
                    })
            }) {
                found |= Capability::PlayerAttachments.bit();
            }
        }
        // An ability a permanent is granted reads what one of its own does.
        if face.grants.iter().any(|a| a.layer().is_some()) {
            found |= Capability::GrantsLayerEffect.bit();
        }
        for a in face.abilities.iter().chain(face.grants) {
            if a.chapter_numbers().is_some() {
                found |= Capability::Saga.bit();
            }
            if let Some(replacement) = a.replacement() {
                found |= replacement_capabilities(replacement);
            }
            if let Some(spec) = a.layer() {
                if matches!(spec.affected, beryl_card::Objects::Everywhere(_)) {
                    found |= Capability::ReachesElsewhere.bit();
                }
                if (spec.modifications.iter())
                    .any(|m| matches!(m, Modification::SetPower(_) | Modification::SetPt(..)))
                {
                    found |= Capability::DefinesPt.bit();
                }
            }
            if a.spend().is_some() {
                found |= Capability::RemembersMana.bit();
            }
            if a.companion_condition().is_some() {
                found |= Capability::Companion.bit();
            }
            if a.graveyard_cast().is_some() {
                found |= Capability::CastsFromGraveyard.bit();
            }
            found |= a.bands_with().map_or(0, |_| Capability::Banding.bit());
            if a.rule()
                .is_some_and(|rule| matches!(rule.what, beryl_card::Governed::TakeStep(_)))
            {
                found |= Capability::TurnStructure.bit();
            }
            if matches!(
                a.designation(),
                Designation::LevelUp(_) | Designation::AtLevel(_) | Designation::Solved
            ) {
                found |= Capability::DesignatedAbilities.bit();
            }
            found |= body_capabilities(a.body());
            if a.cost().is_loyalty() {
                found |= Capability::LoyaltyActivated.bit();
            }
            if a.kind() == AbilityKind::Mana && a.asks() {
                found |= Capability::ManaAbilityChoices.bit();
            }
            if crate::table::is_instant_mana(a) {
                found |= Capability::InstantMana.bit();
            }
            if a.kind() == AbilityKind::Activated
                && (a.zones().contains(Zone::Graveyard) || a.zones().contains(Zone::Exile))
            {
                found |= Capability::ActivatesFromGraveyardOrExile.bit();
            }
            found |= facts_read_by(a);
        }
        found
    }

    #[inline]
    pub(super) const fn bit(self) -> u64 {
        1 << (self as u64)
    }
}

impl super::GameTable {
    /// This table with its capabilities found again as if the cards at `without` were in no
    /// deck: the same cards under the same indices, so a state of this table is a state of
    /// the copy. Everything else the table found from those cards stays, such as their
    /// listeners and rule changes. The present-cost benchmark measures a family of cards
    /// against a copy without the paths their capabilities switch on (`beryl_bench`'s
    /// `family`).
    #[must_use]
    pub fn without_capabilities_of(&self, without: &[crate::ids::CardIx]) -> super::GameTable {
        let kept: Vec<&CardDef> = (self.defs.iter().enumerate())
            .filter(|&(i, _)| !without.iter().any(|card| card.index() == i))
            .map(|(_, &def)| def)
            .collect();
        let capabilities = Capability::of_game(&kept, &self.subtypes);
        super::GameTable {
            capabilities,
            ..self.clone()
        }
    }
}

/// The capabilities a replacement effect gives a game. Each test reads a different field of the
/// replacement, so they are separate tests: as arms of one match, the first to accept a
/// replacement would hide the others.
fn replacement_capabilities(replacement: &Replacement) -> u64 {
    let mut found = 0;
    if matches!(replacement.rewrite, Rewrite::Dredge(_)) {
        found |= Capability::Dredge.bit();
    }
    if matches!(
        replacement.replaces,
        Replaced::Draw(_, Draws::ExceptFirstInDrawStep)
    ) {
        found |= Capability::DrewInDrawStep.bit();
    }
    if matches!(
        replacement.rewrite,
        Rewrite::Choose(_) | Rewrite::EnterAsCopy { .. }
    ) {
        found |= Capability::ChoosesAsEnters.bit();
    }
    found
}

/// The capabilities that come from a face's keyword abilities.
fn keyword_capabilities(keywords: Keywords) -> u64 {
    let mut found = 0;
    if keywords.contains(Keywords::DELVE) {
        found |= Capability::Delve.bit();
    }
    if keywords.contains(Keywords::BANDING) {
        found |= Capability::Banding.bit();
    }
    found
}

/// The capabilities that come from a face's casting options, costs and permissions.
fn casting_capabilities(face: &Face) -> u64 {
    use beryl_card::CastingOption;

    let mut found = if face.permits_cast_while_searching() {
        Capability::CastWhileSearching.bit()
    } else {
        0
    };
    if face
        .additional_cost
        .iter()
        .chain(face.casting.iter().flat_map(|option| option.parts()))
        .any(|part| matches!(part, CostPart::Chosen(CostAction::Sacrifice, _)))
    {
        found |= Capability::SacrificeToCast.bit();
    }
    face.casting.iter().fold(found, |found, option| {
        found
            | match option {
                CastingOption::Mutate { .. } => Capability::Merges.bit(),
                CastingOption::Morph(_) => Capability::Morph.bit(),
                CastingOption::Kicker { .. }
                | CastingOption::Offspring(_)
                | CastingOption::Impending { .. }
                | CastingOption::Overload { .. }
                | CastingOption::Evoke { .. }
                | CastingOption::Alternative { .. }
                | CastingOption::Escape { .. }
                | CastingOption::Fuse
                | CastingOption::Prototype { .. } => 0,
            }
    })
}

/// The capabilities needed by the instructions a spell or ability can carry out.
fn body_capabilities(body: Behavior) -> u64 {
    match body {
        Behavior::Data(effect) => {
            let mut found = 0;
            if effect.any(&|instruction| {
                matches!(
                    instruction,
                    Effect::ExileUntil {
                        matching: beryl_card::ExileMatch::SharesTypeWithTriggeringSpell,
                        ..
                    }
                )
            }) {
                found |= Capability::ReadsDepartedTriggeringSpell.bit();
            }
            if copies_itself(effect) {
                found |= Capability::ReadsDepartedSpell.bit();
            }
            // A card cast face down is turned face up as a morph is, and is told apart from
            // other face-down objects the same way.
            if effect.any(&|instruction| {
                matches!(instruction, Effect::MayCast { face, .. } if *face != beryl_card::CastFace::Up)
            }) {
                found |= Capability::Morph.bit();
            }
            if effect
                .any(&|instruction| matches!(instruction, Effect::MayCast { within_x: true, .. }))
            {
                found |= Capability::SpentOnX.bit();
            }
            if effect.any(&|instruction| {
                matches!(
                    instruction,
                    Effect::EndTurn | Effect::AdditionalStep(_) | Effect::AdditionalPhases { .. }
                ) || matches!(instruction, Effect::ChangeRule { rule, .. }
                        if matches!(rule.what, beryl_card::Governed::TakeStep(_)))
            }) {
                found |= Capability::TurnStructure.bit();
            }
            found
        }
        Behavior::Custom(body) if body.says().changes_turn_structure => {
            Capability::TurnStructure.bit()
        }
        Behavior::Custom(_) => 0,
    }
}

/// The facts an ability reads, whether they appear in its body or in its conditions.
fn facts_read_by(ability: &Ability) -> u64 {
    let mut found = 0;
    if ability
        .trigger()
        .is_some_and(|trigger| trigger.cast_number.is_some())
    {
        found |= Capability::SpellsCast.bit();
    }
    let works_on_stack =
        ability.kind() == AbilityKind::Triggered && ability.zones().contains(Zone::Stack);
    ability.walk(&mut |node| {
        found |= match node {
            Node::Condition(Condition::EnteredThisTurn(_)) => Capability::EnteredThisTurn.bit(),
            Node::Amount(Amount::SpellsCast(_) | Amount::SpellsCastBefore) => {
                Capability::SpellsCast.bit()
            }
            Node::Amount(Amount::ManaValue(Objects::This)) if works_on_stack => {
                Capability::ReadsDepartedSpell.bit()
            }
            Node::Amount(Amount::CardsDrawn(_)) => Capability::CardsDrawn.bit(),
            Node::Condition(Condition::DrewInDrawStep(_)) => Capability::DrewInDrawStep.bit(),
            Node::Filter(Filter::AttackedThisTurn) => Capability::AttackedThisTurn.bit(),
            Node::Condition(Condition::Cast(_)) => Capability::RemembersCast.bit(),
            Node::Filter(Filter::ManaValueAtMostPower) => Capability::ReadsSourcePower.bit(),
            Node::Filter(_) | Node::Amount(_) | Node::Condition(_) | Node::Objects(_) => 0,
        };
    });
    found
}

/// Whether `effect`, or an instruction it holds, copies the spell whose ability it is.
fn copies_itself(effect: &Effect) -> bool {
    effect.any(&|instruction| {
        matches!(
            instruction,
            Effect::CopySpell {
                spells: Objects::This,
                ..
            }
        )
    })
}

/// The event kinds trigger matching looks for away from the faces of the battlefield's
/// definitions: `away`, and in a game with mutate (`capabilities` has
/// [`Capability::Merges`]) every kind a permanent's ability listens to, `here`. A card of any
/// face may be under a merged permanent, where the table's listeners of the permanent's
/// definition do not find its triggered abilities, so trigger matching looks for them apart.
pub(super) fn merged_listen_away(capabilities: u64, away: Listens, here: Listens) -> Listens {
    if capabilities & Capability::Merges.bit() == 0 {
        away
    } else {
        away | here
    }
}
