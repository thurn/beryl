//! The [`Effect`] enum: every instruction a spell or ability can give as it resolves.

use crate::continuous::{ChosenKind, Duration, Modification, RuleChange};
use crate::cost::Cost;
use crate::counter::CounterKind;
use crate::definition::CardDef;
use crate::filter::Filter;
use crate::ids::{TurnPhase, Zone};
use crate::index::{AbilityIx, ExceptIx};
use crate::mana::{ManaColor, ManaSet};
use crate::prompt::Prompt;

use crate::effect::{
    Amount, CastFace, CastTerms, Condition, ExileMatch, Found, LibraryPosition, LibraryRest,
    Objects, Players, Prevention, PutOnto, Recipients, Regeneration, Taken,
};

/// One instruction, or several composed
/// ([CR 609.1](https://yawgatog.com/resources/magic-rules/#R6091)): "An effect is something
/// that happens in the game as a result of a spell or ability."
///
/// An instruction that cannot be done in full does what it can
/// ([CR 609.3](https://yawgatog.com/resources/magic-rules/#R6093)): "If an effect attempts to
/// do something impossible, it does only as much as possible." One whose target has become
/// illegal does nothing to that target (CR 608.2b), so [`Objects::Target`] then names
/// nothing.
#[derive(Clone, Copy, Debug)]
pub enum Effect {
    /// Each instruction in turn
    /// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)): "The controller of
    /// the spell or ability follows its instructions in the order written." None is no
    /// instruction at all ([`Effect::NOTHING`]).
    Sequence(&'static [Effect]),
    /// "If [condition], [then]. Otherwise, [otherwise]." The condition is read as the
    /// instruction is reached ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)):
    /// "If an effect requires information from the game ..., the answer is determined only
    /// once, when the effect is applied."
    If {
        condition: Condition,
        then: &'static Effect,
        otherwise: Option<&'static Effect>,
    },
    /// "[Player] may [effect]": the player chooses as the instruction is reached
    /// ([CR 608.2d](https://yawgatog.com/resources/magic-rules/#R6082d)): "If an effect of a spell
    /// or ability offers any choices ..., the player announces these while applying the effect."
    May {
        player: Players,
        effect: &'static Effect,
    },
    /// "[Effect] unless [player] pays [cost]", or "unless [player] [does something else]"
    /// ([CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a)): "This means the
    /// same thing as '[A player may do something else]. If [that player doesn't], [do
    /// something].'" What the player may do is a cost
    /// ([CR 118.12](https://yawgatog.com/resources/magic-rules/#R11812)): "The action [do
    /// something] is a cost, paid when the spell or ability resolves." A player who cannot
    /// pay is not asked ([CR 118.3](https://yawgatog.com/resources/magic-rules/#R1183)).
    ///
    /// The engine takes two costs as an effect resolves: mana alone, and the discard of one
    /// card from the player's hand that a filter accepts
    /// ([`crate::Malformed::Cost`]).
    Unless {
        effect: &'static Effect,
        player: Players,
        cost: Cost,
    },
    /// "Each [player] [effect]", where the effect differs by player: within it,
    /// [`Players::ThatPlayer`] is the player it is being done for. The players take their
    /// turns at it in turn order from the active player
    /// ([CR 101.4](https://yawgatog.com/resources/magic-rules/#R1014)). An instruction that
    /// is the same for every player takes the players as its operand instead.
    ForEachPlayer {
        players: Players,
        effect: &'static Effect,
    },
    /// "[Player] chooses one of [objects]", as the instruction is reached (CR 608.2d): what
    /// "a card exiled with this" or "a nonland card from it" says before the card says what
    /// is done with it, which the instruction after does to [`Objects::Those`]. With
    /// `optional`, "you may": the player may choose none. With none to choose among, no one
    /// is asked and nothing is chosen
    /// ([CR 609.3](https://yawgatog.com/resources/magic-rules/#R6093)).
    ///
    /// The `prompt` is what the choice is for ([`Prompt`]): a pick of one object.
    ///
    /// With `hidden`, the pick is not revealed to the other players as it is made
    /// ([`Effect::hidden_pick`] says how it is asked).
    Choose {
        player: Players,
        among: Objects,
        prompt: Prompt,
        optional: bool,
        hidden: bool,
    },
    /// "[Player] chooses a [kind]", as the instruction is reached (CR 608.2d), as in "Choose a
    /// card name, then target player mills a card". The resolution remembers the value until it
    /// ends, and a later instruction of it reads "the chosen name"
    /// ([`Condition::HasChosenName`]). A card name is one of the game's cards or "another
    /// card" ([CR 201.4](https://yawgatog.com/resources/magic-rules/#R2014)): "If an effect
    /// instructs a player to choose a card name, the player must choose the name of a card in
    /// the Oracle card reference." The choice is made in public.
    ChooseValue { player: Players, kind: ChosenKind },
    /// One object chosen uniformly at random among `[objects]` as the instruction is reached,
    /// which the instruction after acts on as [`Objects::Those`]: "a creature card at random
    /// from your graveyard". With none to choose among, nothing is chosen
    /// ([CR 609.3](https://yawgatog.com/resources/magic-rules/#R6093)).
    ChooseAtRandom(Objects),
    /// "[Player] looks at [cards]", cards of a hidden zone
    /// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)): "Hidden zones are
    /// zones in which not all players can be expected to see the cards' faces." The player
    /// knows them from then on; no one else learns anything.
    LookAt { player: Players, cards: Objects },
    /// "Counter [spell]"
    /// ([CR 701.6a](https://yawgatog.com/resources/magic-rules/#R7016a)): "To counter a spell
    /// or ability means to cancel it, removing it from the stack. It doesn't resolve and none
    /// of its effects occur. A countered spell is put into its owner's graveyard."
    Counter(Objects),
    /// "Counter [spell]. If that spell is countered this way, [put it into a zone] instead of
    /// putting it into its owner's graveyard": the second sentence changes where the first
    /// puts it ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)): "later text
    /// on the card may modify the meaning of earlier text". The engine puts a countered spell
    /// into exile or on top of its owner's library (`Zone::Exile` or `Zone::Library`) and
    /// nowhere else ([`crate::Malformed::Countered`]).
    CounterInto { spells: Objects, zone: Zone },
    /// "Copy [spells][ [count] times][, except ...]. You may choose new targets for the copy"
    /// ([CR 707.10]): "To copy a spell, activated ability, or triggered ability means to put a
    /// copy of it onto the stack; a copy of a spell isn't cast ... A copy of a spell or ability
    /// copies both the characteristics of the spell or ability and all decisions made for it,
    /// including modes, targets, the value of X, and additional or alternative costs." The
    /// ability's controller controls and owns the copies. `except` names an exception of the
    /// ability's own face ([`crate::Face::excepts`]), as Fork's "except that the copy is red".
    /// With `new_targets`, the controller may choose new targets for each copy as it is made
    /// ([CR 707.10c]): "The player may leave any number of the targets unchanged, even if those
    /// targets would be illegal. If the player chooses to change some or all of the targets,
    /// the new targets must be legal." A spell that has left the stack is not copied, except
    /// the ability's own spell ([`Objects::This`], as storm's), which is copied as it last
    /// existed there.
    ///
    /// [CR 707.10]: https://yawgatog.com/resources/magic-rules/#R70710
    /// [CR 707.10c]: https://yawgatog.com/resources/magic-rules/#R70710c
    CopySpell {
        spells: Objects,
        count: Amount,
        except: Option<ExceptIx>,
        new_targets: bool,
    },
    /// "Destroy [permanents]"
    /// ([CR 701.8a](https://yawgatog.com/resources/magic-rules/#R7018a)): "To destroy a
    /// permanent, move it from the battlefield to its owner's graveyard."
    Destroy {
        objects: Objects,
        /// Whether a regeneration shield may replace the destruction.
        regeneration: Regeneration,
    },
    /// "Regenerate [permanents]": the next time each would be destroyed this turn, a
    /// regeneration shield replaces the destruction
    /// ([CR 701.19a](https://yawgatog.com/resources/magic-rules/#R70119a)): "If the effect of a
    /// resolving spell or ability regenerates a permanent, it creates a replacement effect that
    /// protects the permanent the next time it would be destroyed this turn."
    Regenerate(Objects),
    /// "Tap [permanents]" ([CR 701.26a]).
    ///
    /// [CR 701.26a]: https://yawgatog.com/resources/magic-rules/#R70126a
    Tap(Objects),
    /// "[Permanents] phase out" ([CR 702.26b]): "If a permanent phases out, its status changes to
    /// “phased out.” Except for rules and effects that specifically mention phased-out
    /// permanents, a phased-out permanent is treated as though it does not exist." What is
    /// attached to them phases out with them (CR 702.26g), and they phase in during the untap
    /// step of the player who controlled them ([CR 502.1]).
    ///
    /// [CR 702.26b]: https://yawgatog.com/resources/magic-rules/#R70226b
    /// [CR 502.1]: https://yawgatog.com/resources/magic-rules/#R5021
    PhaseOut(Objects),
    /// Untap every named permanent ([CR 701.26b]).
    ///
    /// [CR 701.26b]: https://yawgatog.com/resources/magic-rules/#R70126b
    Untap(Objects),
    /// "Exile [objects]"
    /// ([CR 701.13a](https://yawgatog.com/resources/magic-rules/#R70113a)): "To exile an
    /// object, move it to the exile zone from wherever it is."
    Exile(Objects),
    /// Exile cards one at a time from the top of the player's library, stopping after
    /// exiling a card that satisfies `matching`, or when the library is empty.
    /// If a card matched, run `when_found` with that card as [`Objects::Those`].
    /// The outcome is the cards from this instruction still in exile.
    ExileUntil {
        player: Players,
        matching: ExileMatch,
        when_found: &'static Effect,
    },
    /// "Exile [objects] until this leaves the battlefield"
    /// ([CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)): "Some one-shot
    /// effects cause an object to change zones 'until' a specified event occurs. A second
    /// one-shot effect is created immediately after the specified event. This second one-shot
    /// effect returns the object to its previous zone."
    ExileUntilThisLeaves(Objects),
    /// "Exile [cards]", by an ability of an object that has another ability referring to the
    /// cards "exiled with" it: the two are linked
    /// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "The second ability
    /// refers only to cards in the exile zone that were put there as a result of an
    /// instruction to exile them in the first ability." Those cards are
    /// [`Objects::ExiledWithThis`].
    ExileLinked(Objects),
    /// "Sacrifice [permanents]": each one's controller does
    /// ([CR 701.21a](https://yawgatog.com/resources/magic-rules/#R70121a)): "To sacrifice a
    /// permanent, its controller moves it from the battlefield directly to its owner's graveyard."
    Sacrifice(Objects),
    /// "Return [objects] to their owners' hands"
    /// ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)): "If an object would
    /// go to any library, graveyard, or hand other than its owner's, it goes to its owner's
    /// corresponding zone."
    ReturnToHand(Objects),
    /// "Put [objects] into their owners' graveyards" (CR 400.3), from wherever they are, by
    /// no keyword action: a card put there from exile was not destroyed, sacrificed, milled
    /// or discarded.
    PutIntoGraveyard(Objects),
    /// "Return [cards] to the battlefield under their owners' control", from whatever zone
    /// each is in
    /// ([CR 110.2a](https://yawgatog.com/resources/magic-rules/#R1102a)): "If an effect
    /// instructs a player to put an object onto the battlefield, that object enters the
    /// battlefield under that player's control unless the effect states otherwise." Each
    /// enters untapped as a new object (CR 400.7).
    ReturnToBattlefield(Objects),
    /// [`PutOnto`]: "return [objects] to the battlefield under your control", tapped or not,
    /// remembered by the ability's source or not. An instant or sorcery card stays where it
    /// is ([CR 400.4a](https://yawgatog.com/resources/magic-rules/#R4004a)): "If an instant
    /// or sorcery card would enter the battlefield, it remains in its previous zone."
    PutOntoBattlefield(&'static PutOnto),
    /// "Return [cards] to the battlefield transformed under their owners' control"
    /// ([CR 712.11a](https://yawgatog.com/resources/magic-rules/#R71211a)): "If a spell or
    /// ability puts a double-faced card onto the battlefield 'transformed' or 'converted,' it
    /// enters the battlefield with its back face up. If a player is instructed to put a card
    /// that isn't a double-faced card onto the battlefield transformed or converted, that
    /// card stays in its current zone." The cards an instruction before exiled.
    ReturnTransformed(Objects),
    /// Turns a transforming permanent to its other face in place (CR 701.27a).
    Transform(Objects),
    /// "Exile them, then meld them into [result]": exiles this permanent and one permanent the
    /// filter accepts that its controller owns and controls and that has the name of the other
    /// card of this card's meld pair ([`crate::definition::Shape::Meld`]), then puts the two
    /// cards onto the battlefield as one permanent with the pair's combined back face up
    /// ([CR 701.42a]). Objects that can't be melded stay in exile ([CR 701.42c]).
    ///
    /// [CR 701.42a]: https://yawgatog.com/resources/magic-rules/#R70142a
    /// [CR 701.42c]: https://yawgatog.com/resources/magic-rules/#R70142c
    Meld(Filter),
    /// Changes a permanent's status to flipped (CR 710.4).
    Flip(Objects),
    /// Shows cards to every player without moving them (CR 701.20a).
    Reveal(Objects),
    /// "Put [objects] into their owners' libraries [position]" (CR 400.3).
    PutIntoLibrary {
        objects: Objects,
        position: LibraryPosition,
    },
    /// "[Player] puts [count] cards from their hand on top of their library in any order",
    /// of their choice, or their whole hand if it has fewer (CR 609.3)
    /// ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)): "If an effect puts two
    /// or more cards in a specific position in a library at the same time, the owner of
    /// those cards may arrange them in any order."
    PutBack { player: Players, count: Amount },
    /// The player chooses any number of hand cards and their order on the library bottom
    /// (see [CR 401.4]). The outcome is the cards put there.
    ///
    /// [CR 401.4]: https://yawgatog.com/resources/magic-rules/#R4014
    PutAnyOnBottom { player: Players },
    /// "[Player] shuffles [cards] into their library": the cards go to their owners'
    /// libraries (CR 400.3) and each of the players shuffles
    /// ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a)): "To shuffle a
    /// library or a face-down pile of cards, randomize the cards within it so that no player
    /// knows their order." A player shuffles though none of the cards were theirs
    /// ([CR 701.24d](https://yawgatog.com/resources/magic-rules/#R70124d)).
    ShuffleInto { player: Players, cards: Objects },
    /// "[Player] looks at the top [look] cards of their library", takes some of them into
    /// their hand ([`Taken`]), puts `top` of them on top of their library in any order, and
    /// puts the rest where [`LibraryRest`] names. Looking at cards of a library is an
    /// exception to [CR 401.2](https://yawgatog.com/resources/magic-rules/#R4012): "Players
    /// can't look at or change the order of cards in a library." With fewer cards in the
    /// library, the player looks at them all and follows the instructions in order for the
    /// cards that are there (CR 609.3).
    Dig {
        player: Players,
        look: Amount,
        take: Taken,
        top: u8,
        rest: LibraryRest,
    },
    /// "[Player] looks at the top [count] cards of [library]'s library and may put them on
    /// the bottom of that library", as to fateseal, of any player's library
    /// ([CR 701.29a](https://yawgatog.com/resources/magic-rules/#R70129a)): "To 'fateseal N'
    /// means to look at the top N cards of an opponent's library, then put any number of them
    /// on the bottom of that library in any order and the rest on top of that library in any
    /// order." The engine fateseals one card ([`crate::Malformed::Fateseal`]).
    Fateseal {
        player: Players,
        library: Players,
        count: Amount,
    },
    /// "[Player] searches their library for [count] [filter] cards, [puts them somewhere],
    /// then shuffles" ([`Found`] says where)
    /// ([CR 701.23a](https://yawgatog.com/resources/magic-rules/#R70123a)): "To search for a
    /// card in a zone, look at all cards in that zone (even if it's a hidden zone) and find
    /// a card that matches the given description." When the search states a quality (a filter
    /// other than [`Filter::Any`]), the player may find fewer, or none
    /// ([CR 701.23b](https://yawgatog.com/resources/magic-rules/#R70123b)); otherwise they must
    /// find as many as the library holds up to `count`. The filter is read for the player
    /// searching. A player told to search for no cards neither searches nor shuffles: "for each
    /// creature exiled this way, its controller searches" has the players it exiled none of do
    /// nothing.
    Search {
        player: Players,
        filter: Filter,
        count: Amount,
        found: Found,
    },
    /// "[Player] draws [count] cards"
    /// ([CR 121.1](https://yawgatog.com/resources/magic-rules/#R1211)): "A player draws a
    /// card by putting the top card of their library into their hand", one at a time
    /// ([CR 121.2](https://yawgatog.com/resources/magic-rules/#R1212)).
    Draw { player: Players, count: Amount },
    /// "[Player] discards [count] cards", of that player's choice
    /// ([CR 701.9a](https://yawgatog.com/resources/magic-rules/#R7019a)): "To discard a card,
    /// move it from its owner's hand to that player's graveyard."
    /// [CR 701.9b](https://yawgatog.com/resources/magic-rules/#R7019b): "By default, effects
    /// that cause a player to discard a card allow the affected player to choose which card
    /// to discard."
    Discard { player: Players, count: Amount },
    /// "[Player] mills [count] cards"
    /// ([CR 701.17a](https://yawgatog.com/resources/magic-rules/#R70117a)): "that player puts
    /// that many cards from the top of their library into their graveyard."
    Mill { player: Players, count: Amount },
    /// "[Player] scries [count]"
    /// ([CR 701.22a](https://yawgatog.com/resources/magic-rules/#R70122a)): "To 'scry N'
    /// means to look at the top N cards of your library, then put any number of them on the
    /// bottom of your library in any order and the rest on top of your library in any order."
    Scry { player: Players, count: Amount },
    /// "[Permanents] explore"
    /// ([CR 701.44a](https://yawgatog.com/resources/magic-rules/#R70144a)): "that permanent's
    /// controller reveals the top card of their library. If a land card is revealed this
    /// way, that player puts that card into their hand. Otherwise, that player puts a +1/+1
    /// counter on the exploring permanent and may put the revealed card into their
    /// graveyard."
    Explore(Objects),
    /// "[Player] manifests dread"
    /// ([CR 701.62a](https://yawgatog.com/resources/magic-rules/#R70162a)): "Look at the top
    /// two cards of your library. Manifest one of them, then put the cards you looked at that
    /// were not manifested this way into your graveyard."
    ManifestDread(Players),
    /// "[Player] flips a coin"
    /// ([CR 705.2](https://yawgatog.com/resources/magic-rules/#R7052)): "the player that
    /// flips the coin calls 'heads' or 'tails.' If the call matches the result, the player
    /// wins the flip. Otherwise, the player loses the flip." Whether they won is what
    /// [`Condition::WonFlip`] then reads.
    FlipCoin(Players),
    /// "Players play a Magic subgame, using their libraries as their decks." The players
    /// who did not win are available to the next instruction as [`Players::SubgameNonwinners`].
    /// Under [CR 730.1a], "The main game is temporarily discontinued while the subgame is in
    /// progress. It resumes when the subgame ends."
    ///
    /// [CR 730.1a]: https://yawgatog.com/resources/magic-rules/#R7301a
    Subgame,
    /// "This deals [amount] damage to [recipients]", to each of them
    /// ([CR 120.1](https://yawgatog.com/resources/magic-rules/#R1201)): "Objects can deal
    /// damage to battles, creatures, planeswalkers, and players." The source is the object
    /// the ability is on, or the spell
    /// ([CR 120.2b](https://yawgatog.com/resources/magic-rules/#R1202b)).
    DealDamage { to: Recipients, amount: Amount },
    /// A prevention shield on each of the recipients for the rest of the turn
    /// ([CR 615.1a](https://yawgatog.com/resources/magic-rules/#R6151a)): "Effects that use
    /// the word 'prevent' are prevention effects. Prevention effects use 'prevent' to indicate
    /// what damage will not be dealt." It prevents what `prevention` says of the next damage
    /// dealt to the recipient this turn. With `from`, only damage from a source of the
    /// controller's choice that the filter accepts as the instruction is carried out: "the
    /// next time a red source of your choice would deal damage to you this turn". The source
    /// is chosen then and is not a target
    /// ([CR 609.7a](https://yawgatog.com/resources/magic-rules/#R6097a)): "If an effect
    /// requires a player to choose a source of damage, they may choose a permanent; a spell
    /// on the stack (including a permanent spell); any object referred to by an object on the
    /// stack".
    PreventDamage {
        to: Recipients,
        prevention: Prevention,
        from: Option<&'static Filter>,
    },
    /// "[Player] gains [amount] life"
    /// ([CR 119.3](https://yawgatog.com/resources/magic-rules/#R1193)): "If an effect causes
    /// a player to gain life or lose life, that player's life total is adjusted accordingly."
    GainLife { player: Players, amount: Amount },
    /// "[Player] loses [amount] life" (CR 119.3).
    LoseLife { player: Players, amount: Amount },
    /// "[Player] creates [count] [token] tokens"
    /// ([CR 701.7a](https://yawgatog.com/resources/magic-rules/#R7017a)): "To create one or
    /// more tokens with certain characteristics, put the specified number of tokens with the
    /// specified characteristics onto the battlefield."
    /// [CR 111.2](https://yawgatog.com/resources/magic-rules/#R1112): "The player who creates
    /// a token is its owner. The token enters the battlefield under that player's control."
    /// The token's definition is its text
    /// ([CR 111.3](https://yawgatog.com/resources/magic-rules/#R1113)).
    CreateTokens {
        player: Players,
        token: &'static CardDef,
        count: Amount,
    },
    /// "[Player] creates [count] [token] tokens that are tapped and attacking [players] or a
    /// planeswalker they control"
    /// ([CR 508.4](https://yawgatog.com/resources/magic-rules/#R5084)): "If a creature is put
    /// onto the battlefield attacking, its controller chooses which defending player,
    /// planeswalker a defending player controls, or battle a defending player protects it's
    /// attacking as it enters the battlefield (unless the effect that put it onto the
    /// battlefield specifies what it's attacking)." A token whose controller is not attacking
    /// one of those players enters tapped and attacks nothing (CR 506.3b, CR 506.3c).
    CreateAttackingTokens {
        player: Players,
        token: &'static CardDef,
        count: Amount,
        attacking: Players,
    },
    /// "[Player] creates [count] tokens that are copies of [object]"
    /// ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)): "When copying an
    /// object, the copy acquires the copiable values of the original object's
    /// characteristics".
    /// A target the effect has moved is copied as it last was
    /// ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "the effect uses
    /// the object's last known information."
    CreateCopies {
        player: Players,
        of: Objects,
        count: Amount,
        /// The exception of the ability's own face the copies are made with
        /// ([`crate::Face::excepts`]), or none for exact copies.
        except: Option<ExceptIx>,
    },
    /// "[Objects] become a copy of [of]": each permanent takes the copiable values of the one
    /// it copies, with the exception at `except` of the ability's own face
    /// ([`crate::Face::excepts`]) after them ([CR 707.2]): "When copying an object, the copy
    /// acquires the copiable values of the original object's characteristics". Those values
    /// are what is printed on it, as other copy effects modify them, not what other effects
    /// make it ([CR 707.2]). Becoming a copy is not entering the battlefield: the permanent
    /// keeps its counters, its status and what it remembers, and nothing that happens as an
    /// object enters applies (a permanent that becomes a copy of Dark Depths has no ice
    /// counters). The copy effect lasts until the permanent leaves the battlefield or a later
    /// copy effect overrides it ([CR 613.2a]), as a copy effect without a duration does.
    /// Nothing happens if what it copies has left the battlefield.
    ///
    /// [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
    /// [CR 613.2a]: https://yawgatog.com/resources/magic-rules/#R6132a
    BecomeCopy {
        objects: Objects,
        of: Objects,
        except: Option<ExceptIx>,
    },
    /// A mutating creature spell merges with [objects], its target ([CR 702.140c]): "it merges
    /// with the target creature ... The spell's controller chooses whether the spell is put on
    /// top of the creature or on the bottom." The spell ability only a cast for
    /// [`crate::CastingOption::Mutate`] has.
    ///
    /// [CR 702.140c]: https://yawgatog.com/resources/magic-rules/#R702140c
    Mutate(Objects),
    /// "Put [count] [kind] counters on [objects]"
    /// ([CR 122.1](https://yawgatog.com/resources/magic-rules/#R1221)): "A counter is a
    /// marker placed on an object or player that modifies its characteristics and/or
    /// interacts with a rule, ability, or effect."
    AddCounters {
        objects: Objects,
        kind: CounterKind,
        count: Amount,
    },
    /// "Remove [count] [kind] counters from [objects]" (CR 122.1), or as many as each has
    /// (CR 609.3).
    RemoveCounters {
        objects: Objects,
        kind: CounterKind,
        count: Amount,
    },
    /// "Remove all counters from [objects]": every counter of every kind (CR 122.1).
    RemoveAllCounters(Objects),
    /// "[Player] adds [mana]"
    /// ([CR 106.3](https://yawgatog.com/resources/magic-rules/#R1063)): "A spell or ability
    /// that produces mana instructs a player to add that mana." Where the set has several
    /// kinds, the player chooses one of them.
    AddMana { player: Players, mana: ManaSet },
    /// "[Player] adds [amount] mana of [kind]", by a spell or an ability that is no mana
    /// ability, as "add an amount of {C} equal to that spell's mana value"
    /// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect
    /// instructs a player to add mana, that mana goes into a player's mana pool." The kind
    /// is the one it names: no one chooses.
    AddManaOfKind {
        player: Players,
        kind: ManaColor,
        amount: Amount,
    },
    /// "[Player] adds [the mana ability's product]": the effect of a mana ability whose
    /// product is more than one fixed amount of one kind or one color
    /// ([`crate::Ability::mana_of`]), as "Add {W}{U}.", "Add {B} for each Swamp you
    /// control." or "Add {W}{W}, {W}{U}, or {U}{U}." say
    /// ([CR 106.3](https://yawgatog.com/resources/magic-rules/#R1063)): "A spell or ability
    /// that produces mana instructs a player to add that mana." The terms are the ability's
    /// ([`crate::Ability::product`]), each of whose kinds is chosen as [`crate::Term`] says:
    /// an effect is held by reference, and a `const fn` cannot make a reference to an effect
    /// that holds its argument, so the effect names no terms of its own.
    AddProduct { player: Players },
    /// "[Objects] get, gain or become [modifications] [duration]": a continuous effect from a
    /// resolving spell or ability
    /// ([CR 611.2a](https://yawgatog.com/resources/magic-rules/#R6112a)): it "lasts as long as
    /// stated by the spell or ability creating it (such as 'until end of turn')." What it
    /// affects is settled as it begins
    /// ([CR 611.2c](https://yawgatog.com/resources/magic-rules/#R6112c)): "the set of objects
    /// it affects is determined when that continuous effect begins. After that point, the set
    /// won't change."
    /// Its modifications are the parts of one effect, which begin together with one timestamp
    /// and each apply in their own layer
    /// ([CR 613.6](https://yawgatog.com/resources/magic-rules/#R6136)): "If an effect should
    /// be applied in different layers and/or sublayers, the parts of the effect each apply in
    /// their appropriate ones."
    Modify {
        objects: Objects,
        modifications: &'static [Modification],
        duration: Duration,
    },
    /// "Change the text of [objects] by replacing all instances of [words] [duration]",
    /// where the words are "one color word with another or one basic land type with
    /// another" or one half of that ([`crate::TextWords`]): a text-changing effect
    /// ([CR 612.1]): "Some continuous effects change an object’s text. This can apply to any
    /// words or symbols printed on that object". Its controller chooses the two words as it
    /// resolves (CR 608.2d), and the effect is on each object, a spell or a permanent, as it
    /// is then. Only the object's own text changes ([CR 612.3]): "any abilities that are
    /// granted to an object can't be modified by text-changing effects that affect that
    /// object".
    ///
    /// [CR 612.1]: https://yawgatog.com/resources/magic-rules/#R6121
    /// [CR 612.3]: https://yawgatog.com/resources/magic-rules/#R6123
    ChangeText {
        objects: Objects,
        words: crate::TextWords,
        duration: Duration,
    },
    /// "Exchange control of [first] and [second]": each permanent gets the other's controller,
    /// together, for the rest of the game, unless one side names no permanent
    /// ([CR 701.12a](https://yawgatog.com/resources/magic-rules/#R70112a)): "if the entire
    /// exchange can't be completed, no part of the exchange occurs."
    ExchangeControl(Objects, Objects),
    /// "Attach [objects] to [host]": an Equipment the objects name moves onto the one
    /// creature `to` names
    /// ([CR 701.3a](https://yawgatog.com/resources/magic-rules/#R7013a)): "To attach an Aura,
    /// Equipment, or Fortification to an object or player means to take it from where it
    /// currently is and put it onto that object or player." Equip's ability says it
    /// ([`crate::Ability::equip`]), and so does Thieving Skydiver's "attach it to this
    /// creature".
    Attach { objects: Objects, to: Objects },
    /// "[Rule change] [duration]": a continuous effect from a resolving spell or ability that
    /// changes a rule of the game for the players it names
    /// ([CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311)): "Some continuous
    /// effects affect game rules rather than objects." An ability starts at most one
    /// ([`crate::Malformed::Rules`]), which is its [`crate::Ability::rule`].
    ChangeRule {
        rule: &'static RuleChange,
        duration: Duration,
    },
    /// "[Cards] gain flashback until end of turn. The flashback cost is equal to its mana
    /// cost": a keyword ability granted to a card that is no permanent
    /// ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a)): "'Flashback
    /// [cost]' means 'You may cast this card from your graveyard if the resulting spell is an
    /// instant or sorcery spell by paying [cost] rather than paying its mana cost' and 'If the
    /// flashback cost was paid, exile this card instead of putting it anywhere else any time
    /// it would leave the stack.'" Each card's owner is the player who may cast it.
    GainFlashback(Objects),
    /// "[Player] may cast [cards] [duration]": a permission
    /// ([CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013)): "A player can begin
    /// to cast a spell only if a rule or effect allows that player to cast it and no rule or
    /// effect prohibits that player from casting it." It is about each card where it is, so
    /// it ends when the card leaves that zone
    /// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)), which is the whole of
    /// "for as long as that card remains exiled" ([`Duration::Indefinite`]); "this turn" is
    /// [`Duration::EndOfTurn`]. What it says of a spell cast this way is its `terms`.
    PermitCast {
        player: Players,
        cards: Objects,
        duration: Duration,
        terms: CastTerms,
    },
    /// "[Player] may cast [card]", and with `free`, "without paying its mana cost"
    /// ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)): "If an effect
    /// specifically instructs or allows a player to cast a spell during resolution, they do
    /// so by following the steps in rules 601.2a-i, except no player receives priority after
    /// it's cast." The player may choose one cast from the cards the expression names.
    /// Without its mana cost the card is cast for an alternative cost
    /// ([CR 118.9](https://yawgatog.com/resources/magic-rules/#R1189)). The free cast can
    /// require the resulting spell's mana value to be less than `free_below`.
    ///
    /// With `copy`, "copy [card]. [Player] may cast the copy": the player chooses among the
    /// cards, and a copy of the card chosen is made in its zone and cast ([CR 707.12]): "the copy
    /// is created in the same zone the object is in and then cast while another spell or
    /// ability is resolving." A copy not cast would cease to exist at once, so none is made
    /// for a player who declines.
    ///
    /// `face` says which way up the card is cast ([`CastFace`]). With `within_x`, only a card
    /// "whose mana cost could be paid by some amount of, or all of, the mana you spent on
    /// {X}" is offered: the activated ability resolving had {X} in its cost, and both the
    /// amount and the kinds of mana spent on it count.
    ///
    /// [CR 707.12]: https://yawgatog.com/resources/magic-rules/#R70712
    MayCast {
        player: Players,
        card: Objects,
        free: bool,
        free_below: Option<Amount>,
        copy: bool,
        face: CastFace,
        within_x: bool,
    },
    /// "[Player] untaps up to [count] [permanents]", of their choice as the instruction is
    /// reached
    /// ([CR 701.26b](https://yawgatog.com/resources/magic-rules/#R70126b)): "To untap a
    /// permanent, rotate it back to the upright position from a sideways position. Only
    /// tapped permanents can be untapped." The permanents are any the expression names,
    /// whoever controls them.
    UntapUpTo {
        player: Players,
        count: Amount,
        permanents: Objects,
    },
    /// "[When], [effect]", said in the middle of an effect: creates a delayed triggered
    /// ability
    /// ([CR 603.7a](https://yawgatog.com/resources/magic-rules/#R6037a)): "Delayed triggered
    /// abilities are created during the resolution of spells or abilities". Its controller
    /// is the player who controls what is resolving
    /// ([CR 603.7d](https://yawgatog.com/resources/magic-rules/#R6037d),
    /// [CR 603.7e](https://yawgatog.com/resources/magic-rules/#R6037e)), and its source is
    /// that spell or that ability's source.
    ///
    /// The ability is one of its face's, written with a delayed trigger
    /// ([`crate::Trigger::next`]) and named here by its place among them: a game keeps a
    /// waiting ability as its card, face and place. `carrying` is a number (none carries 0)
    /// settled as this instruction is reached
    /// ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)), which the delayed
    /// ability reads when it resolves, as "add an amount of {C} equal to that spell's mana
    /// value". `object`, settled the same way, is the one object the delayed ability names
    /// as [`Objects::Carried`]: the first of what the expression names, as it is then.
    Delay {
        ability: AbilityIx,
        carrying: Option<Amount>,
        object: Option<Objects>,
    },
    /// "[Player] gets an emblem with [abilities]"
    /// ([CR 114.2](https://yawgatog.com/resources/magic-rules/#R1142)): "This means that
    /// [player] puts an emblem with [ability] into the command zone. The emblem is both
    /// owned and controlled by that player." The emblem's definition is its abilities; its
    /// card lists it among what it creates ([`CardDef::tokens`]).
    GetEmblem {
        player: Players,
        emblem: &'static CardDef,
    },
    /// "[Player] wins the game"
    /// ([CR 104.2b](https://yawgatog.com/resources/magic-rules/#R1042b)): "An effect may
    /// state that a player wins the game."
    WinGame(Players),
    /// "[Player] loses the game"
    /// ([CR 104.3e](https://yawgatog.com/resources/magic-rules/#R1043e)): "An effect may
    /// state that a player loses the game."
    LoseGame(Players),
    /// "You control [player] during that player's next turn"
    /// ([CR 724.1](https://yawgatog.com/resources/magic-rules/#R7241)): "Some cards allow a
    /// player to control another player during that player's next turn. This effect applies
    /// to the next turn that the affected player actually takes." "You" is the controller of
    /// what is resolving.
    ControlPlayer(Players),
    /// "[Player] takes an extra turn after this one"
    /// ([CR 500.7](https://yawgatog.com/resources/magic-rules/#R5007)): "Some effects can give
    /// a player extra turns. They do this by adding the turns directly after the specified
    /// turn. ... If multiple players are given extra turns, the extra turns are added one at a
    /// time, in APNAP order (see rule 101.4). The most recently created turn will be taken
    /// first."
    /// Optionally creates the delayed ability that will trigger only during that extra turn.
    ExtraTurn(Players, Option<AbilityIx>),
    /// "That player gets an additional [step] after this step."
    /// Adds a step to the active player's current phase (see [CR 500.9]).
    ///
    /// [CR 500.9]: https://yawgatog.com/resources/magic-rules/#R5009
    AdditionalStep(crate::TurnStep),
    /// Adds these phases, in order, immediately after the current phase if it matches `after`
    /// ([CR 500.8]). A spell may do this during another player's turn.
    ///
    /// [CR 500.8]: https://yawgatog.com/resources/magic-rules/#R5008
    AdditionalPhases {
        after: TurnPhase,
        phases: &'static [TurnPhase],
    },
    /// "End the turn" ([CR 725.1]): stop resolving, exile the stack and go to cleanup.
    /// The skipped steps trigger nothing. An ability still waiting for the next end step
    /// waits for the next one that actually begins. Triggers from the turn-ending process
    /// go on the stack in cleanup, where priority is followed by another cleanup step.
    ///
    /// [CR 725.1]: https://yawgatog.com/resources/magic-rules/#R7251
    EndTurn,
    /// Restarts the game and keeps the linked cards that pass the filter in exile.
    /// After mulligans the ability's controller puts those cards onto the battlefield
    /// under their control (see [CR 728.4] and [CR 728.5]).
    ///
    /// [CR 728.4]: https://yawgatog.com/resources/magic-rules/#R7284
    /// [CR 728.5]: https://yawgatog.com/resources/magic-rules/#R7285
    RestartGame { filter: Filter },
    /// The controller may reveal a card they own from outside the game that matches
    /// `filter` and put it into their hand (see CR 400.11b).
    Wish { filter: Filter },
    /// Makes the selected permanents prepared or unprepared (see CR 723.3a–b).
    SetPrepared { objects: Objects, prepared: bool },
}

impl Effect {
    /// No instruction: what an ability that never resolves does, such as a static ability.
    pub const NOTHING: Effect = Effect::Sequence(&[]);
}
