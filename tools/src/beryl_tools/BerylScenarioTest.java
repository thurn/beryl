package org.mage.test.beryl;

import java.io.Serializable;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.UUID;
import java.util.function.Predicate;
import java.util.function.Supplier;
import java.util.stream.Collectors;
import java.util.stream.Stream;

import mage.MageItem;
import mage.MageObject;
import mage.ObjectColor;
import mage.abilities.Ability;
import mage.abilities.keyword.ChangelingAbility;
import mage.abilities.keyword.DelveAbility;
import mage.abilities.keyword.FirstStrikeAbility;
import mage.abilities.keyword.FlashAbility;
import mage.abilities.keyword.FlyingAbility;
import mage.abilities.keyword.ForestwalkAbility;
import mage.abilities.keyword.HasteAbility;
import mage.abilities.keyword.IndestructibleAbility;
import mage.abilities.keyword.IslandwalkAbility;
import mage.abilities.keyword.LifelinkAbility;
import mage.abilities.keyword.MountainwalkAbility;
import mage.abilities.keyword.PlainswalkAbility;
import mage.abilities.keyword.ReachAbility;
import mage.abilities.keyword.SwampwalkAbility;
import mage.abilities.keyword.TrampleAbility;
import mage.abilities.keyword.VigilanceAbility;
import mage.abilities.mana.ActivatedManaAbilityImpl;
import mage.cards.Card;
import mage.cards.Cards;
import mage.choices.Choice;
import mage.constants.ManaType;
import mage.constants.Outcome;
import mage.constants.PhaseStep;
import mage.constants.RangeOfInfluence;
import mage.constants.SubType;
import mage.constants.Zone;
import mage.counters.CounterType;
import mage.game.Game;
import mage.game.permanent.Permanent;
import mage.game.permanent.PermanentToken;
import mage.game.stack.Spell;
import mage.game.stack.StackObject;
import mage.player.ai.PossibleTargetsSelector;
import mage.players.Player;
import mage.target.Target;
import mage.target.TargetCard;
import org.junit.Assert;
import org.mage.test.player.PlayerAction;
import org.mage.test.player.TestComputerPlayer;
import org.mage.test.player.TestPlayer;
import org.mage.test.serverside.base.CardTestPlayerBase;

/**
 * The base of the XMage tests `just oracle` generates from Beryl's scenarios (Beryl's
 * docs/engine-architecture.md, section 17.2): a pass-priority command, and the scenario
 * assertions XMage's own assert family does not state the same way. Each assertion reads the
 * live game, so a test runs it inside `runCode` at the turn and step where Beryl asserted it.
 * Written into the XMage checkout by tools/src/beryl_tools/xmage.py; do not edit it there.
 */
public abstract class BerylScenarioTest extends CardTestPlayerBase {

    private static final String PASS = "beryl:pass";
    private static final String RESOLVE = "beryl:resolve";
    private static final String OPTIONAL = "beryl:optional:";
    private static final String TAP = "beryl:tap:";
    private static final String NORMAL_COST = "Cast with no";

    /** The answer to an alternative-cost menu belongs to the cast that can open it. */
    private static final class CastWithCost extends PlayerAction {
        private final String cost;

        CastWithCost(PlayerAction cast, String cost) {
            super(cast.getActionName(), cast.getTurnNum(), cast.getStep(), cast.getAction());
            this.cost = cost;
        }
    }

    /**
     * A test player that also takes `passPriority`, `resolveTop` and `setOptionalChoice`. XMage runs a command
     * as soon as its player holds priority, so a command Beryl made after the other player's
     * spell resolved would otherwise run before that spell was cast. An optional choice answers a
     * question Beryl answered without asking, or did not ask, which XMage may or may not ask: it is used if the
     * question comes, and dropped at its player's first priority after the step where Beryl
     * answered it if not.
     */
    /** XMage's question whether a loop of the game is a draw. */
    private static final String INFINITE_LOOP = "Draw game because of infinite looping?";

    private static final class BerylTestPlayer extends TestPlayer {
        private String castCost;

        BerylTestPlayer(TestComputerPlayer computerPlayer) {
            super(computerPlayer);
        }

        BerylTestPlayer(final BerylTestPlayer player) {
            super(player);
        }

        @Override
        public BerylTestPlayer copy() {
            return new BerylTestPlayer(this);
        }

        /**
         * Asks `question` with the optional choices in the queue made ordinary ones, and makes
         * those it did not use optional again, as when XMage settles a choice of one card
         * itself. The leading optional choices that `fits` says this question cannot take wait
         * outside the queue while it is asked: their questions were settled unasked, as a
         * discard of the only card in a hand before a yes or no. An optional choice reads
         * `beryl:optional:<turn>:<step>:<choice>`, with the turn and step where Beryl answered it.
         */
        private <T> T withOptional(Predicate<String> fits, Supplier<T> question) {
            List<String> choices = getChoices();
            List<String> bypassed = new ArrayList<>();
            while (!choices.isEmpty() && choices.get(0).startsWith(OPTIONAL)
                    && !fits.test(optionalValue(choices.get(0)))) {
                bypassed.add(choices.remove(0));
            }
            // The question takes choices from the front, so each optional one is kept by its
            // distance from the end of the queue.
            Map<Integer, String> optional = new HashMap<>();
            int size = choices.size();
            for (int i = 0; i < size; i++) {
                String choice = choices.get(i);
                if (choice.startsWith(OPTIONAL)) {
                    optional.put(size - i, choice);
                    choices.set(i, optionalValue(choice));
                }
            }
            try {
                return question.get();
            } finally {
                int left = choices.size();
                for (Map.Entry<Integer, String> entry : optional.entrySet()) {
                    if (entry.getKey() <= left) {
                        choices.set(left - entry.getKey(), entry.getValue());
                    }
                }
                choices.addAll(0, bypassed);
            }
        }

        private static String optionalValue(String choice) {
            return choice.split(":", 5)[4];
        }

        /** Whether `name` names one of the objects or players `target` can choose. */
        private boolean selectable(String name, Outcome outcome, Target target, Ability source, Game game, Cards cards) {
            PossibleTargetsSelector selector = new PossibleTargetsSelector(
                    outcome, target, target.getAffectedAbilityControllerId(getId()), source, game);
            selector.findNewTargets(cards);
            for (MageItem item : selector.getAny()) {
                if (item instanceof MageObject && ((MageObject) item).getName().equals(name)) {
                    return true;
                }
                if (item instanceof Player && ((Player) item).getName().equals(name)) {
                    return true;
                }
            }
            return false;
        }

        /** Whether an optional choice's question would have come by now. */
        private static boolean overdue(String choice, Game game) {
            if (!choice.startsWith(OPTIONAL)) {
                return false;
            }
            String[] parts = choice.split(":", 5);
            int turn = Integer.parseInt(parts[2]);
            int step = PhaseStep.valueOf(parts[3]).getIndex();
            return game.getTurnNum() > turn
                    || (game.getTurnNum() == turn && game.getTurnStepType().getIndex() > step);
        }

        /**
         * A choice from the queue, but with the queue empty, the only candidate of a choice
         * that needs one: Beryl answers a question with one answer without asking, and records
         * it in the scenario only where it asks. An Aura that a loop returns again and again
         * chooses what it enchants each time (CR 303.4f), more times than any queue holds.
         */
        @Override
        public boolean choose(Outcome outcome, Target target, Ability source, Game game, Map<String, Serializable> options) {
            if (getChoices().isEmpty() && target.getMinNumberOfTargets() > 0) {
                Set<UUID> possible = target.possibleTargets(getId(), source, game);
                if (possible.size() == 1) {
                    target.add(possible.iterator().next(), game);
                    return true;
                }
            }
            return withOptional(
                    name -> selectable(name, outcome, target, source, game, null),
                    () -> super.choose(outcome, target, source, game, options));
        }

        @Override
        public boolean choose(Outcome outcome, Cards cards, TargetCard target, Ability source, Game game) {
            return withOptional(
                    name -> selectable(name, outcome, target, source, game, cards),
                    () -> super.choose(outcome, cards, target, source, game));
        }

        @Override
        public boolean choose(Outcome outcome, Choice choice, Game game) {
            if (castCost != null && choice.isKeyChoice()
                    && (choice.getMessage().equals("You may choose an alternative cost")
                        || choice.getMessage().equals("Choose an alternative cost"))) {
                for (Map.Entry<String, String> item : choice.getKeyChoices().entrySet()) {
                    if (item.getValue().startsWith(castCost)) {
                        castCost = null;
                        choice.setChoiceByKey(item.getKey(), false);
                        return true;
                    }
                }
                Assert.fail("Cast cost " + castCost + " is missing from " + choice.getKeyChoices().values());
            }
            return super.choose(outcome, choice, game);
        }

        /**
         * Which of several replacement effects applies first (CR 616.1). Beryl names the card
         * whose effect it chose, which the queue answers as XMage does. It asks nothing where
         * XMage still may: among effects of identically named sources, which do the same, and
         * between an optional effect that works from a graveyard (dredge) and the others, where
         * it asks only the yes or no to the optional effect, which XMage asks again after this
         * (`chooseUse`). A yes applies that effect; a no applies one of the others first, and
         * is used up here.
         */
        @Override
        public int chooseReplacementEffect(Map<String, String> effectsMap, Map<String, MageObject> objectsMap, Game game) {
            List<String> choices = getChoices();
            String next = choices.isEmpty() ? null : choices.get(0);
            boolean named = next != null && effectsMap.values().stream().anyMatch(text -> text.startsWith(next));
            if (effectsMap.size() > 1 && !named) {
                List<String> keys = new ArrayList<>(effectsMap.keySet());
                long sources = keys.stream().map(key -> objectsMap.get(key).getName()).distinct().count();
                if (sources == 1) {
                    return 0;
                }
                if ("Yes".equals(next) || "No".equals(next)) {
                    for (int i = 0; i < keys.size(); i++) {
                        MageObject source = objectsMap.get(keys.get(i));
                        boolean graveyard = game.getState().getZone(source.getId()) == Zone.GRAVEYARD;
                        if (graveyard == "Yes".equals(next)) {
                            if (!graveyard) {
                                choices.remove(0);
                            }
                            return i;
                        }
                    }
                }
            }
            return super.chooseReplacementEffect(effectsMap, objectsMap, game);
        }

        /**
         * A yes or no from the queue, but for XMage's question whether a loop it found is a
         * draw (`GameImpl.checkInfiniteLoop`), always yes: in the rules a loop of mandatory
         * actions is a draw with no player asked (CR 104.4b), and Beryl asks nothing.
         */
        @Override
        public boolean chooseUse(Outcome outcome, String message, String secondMessage, String trueText,
                String falseText, Ability source, Game game) {
            if (INFINITE_LOOP.equals(message)) {
                return true;
            }
            return withOptional(
                    answer -> answer.equals("Yes") || answer.equals("No"),
                    () -> super.chooseUse(outcome, message, secondMessage, trueText, falseText, source, game));
        }

        /** A choice XMage settled silently can still precede the scripted X in the queue. */
        @Override
        public int announceX(int min, int max, String message, Game game, Ability source, boolean isManaPay) {
            return withOptional(answer -> answer.startsWith("X="),
                    () -> super.announceX(min, max, message, game, source, isManaPay));
        }

        /**
         * Activates the mana ability whose text starts with the text after the tab in
         * `command`, of an untapped permanent this player controls named before it. XMage's own
         * `activateManaAbility` takes the first permanent with an ability of that text, which
         * may be another card than Beryl tapped, as a Seachrome Coast for "{T}: Add {W}" where
         * Beryl tapped a Plains.
         */
        private void tap(String command, Game game) {
            String[] parts = command.split("\t", 2);
            for (Permanent permanent : game.getBattlefield().getAllActivePermanents(getId())) {
                if (permanent.isTapped() || !permanent.getName().equals(parts[0])) {
                    continue;
                }
                for (ActivatedManaAbilityImpl ability : permanent.getAbilities(game)
                        .getAvailableActivatedManaAbilities(Zone.BATTLEFIELD, getId(), game)) {
                    if (ability.toString().startsWith(parts[1])) {
                        activateAbility(ability.copy(), game);
                        return;
                    }
                }
            }
            Assert.fail("no untapped " + parts[0] + " of " + getName() + " has " + parts[1]);
        }

        @Override
        public boolean priority(Game game) {
            // The optional choices of questions that never came.
            getChoices().removeIf(choice -> overdue(choice, game));
            // The first command for this turn and step, past the waits that are already over.
            for (PlayerAction action : getActions()) {
                if (action.getTurnNum() != game.getTurnNum() || action.getStep() != game.getTurnStepType()) {
                    continue;
                }
                if (action.getAction().startsWith("waitStackResolved") && game.getStack().isEmpty()) {
                    continue;
                }
                if (action.getAction().equals(PASS)) {
                    getActions().remove(action);
                    pass(game);
                    return false;
                }
                if (action.getAction().equals(RESOLVE)) {
                    if (!game.getStack().isEmpty()) {
                        getActions().remove(action);
                    }
                    pass(game);
                    return false;
                }
                if (action.getAction().startsWith(TAP)) {
                    getActions().remove(action);
                    tap(action.getAction().substring(TAP.length()), game);
                    return true;
                }
                if (action instanceof CastWithCost) {
                    CastWithCost cast = (CastWithCost) action;
                    castCost = cast.cost;
                    try {
                        boolean result = super.priority(game);
                        if (!getActions().contains(cast)) {
                            Assert.assertTrue("XMage did not offer " + castCost,
                                    castCost == null || NORMAL_COST.equals(castCost));
                        }
                        return result;
                    } finally {
                        castCost = null;
                    }
                }
                break;
            }
            return super.priority(game);
        }
    }

    @Override
    protected TestPlayer createNewPlayer(String playerName, RangeOfInfluence rangeOfInfluence) {
        return new BerylTestPlayer(new TestComputerPlayer(playerName, rangeOfInfluence));
    }

    /** `player` passes priority at its first priority in this turn and step. */
    protected void passPriority(int turn, PhaseStep step, TestPlayer player) {
        player.addAction(new PlayerAction("pass priority", turn, step, PASS));
    }

    /**
     * `player` passes priority until the stack has something on it, then once more, to let the
     * top object resolve. XMage's `waitStackResolved` for one object is done at once on an empty
     * stack, before the other player has cast what it was to wait for.
     */
    protected void resolveTop(int turn, PhaseStep step, TestPlayer player) {
        player.addAction(new PlayerAction("resolve the top object", turn, step, RESOLVE));
    }

    /**
     * `player` activates the mana ability whose text starts with `text` of an untapped
     * permanent named `card` that it controls, at its first priority in this turn and step.
     */
    protected void tapSource(int turn, PhaseStep step, TestPlayer player, String card, String text) {
        player.addAction(new PlayerAction("tap " + card, turn, step, TAP + card + "\t" + text));
    }

    /** `choice` answers `player`'s next question, if it comes by the end of `step`. */
    protected void setOptionalChoice(TestPlayer player, int turn, PhaseStep step, String choice) {
        player.addChoice(OPTIONAL + turn + ":" + step.name() + ":" + choice);
    }

    /** Answers the last scheduled cast's alternative-cost menu if XMage opens it. */
    protected void setCastCost(TestPlayer player, String cost) {
        List<PlayerAction> actions = player.getActions();
        int last = actions.size() - 1;
        actions.set(last, new CastWithCost(actions.get(last), cost));
    }

    /**
     * Taps an untapped permanent named `name` that `player` controls when `step` of turn 1
     * comes, without a tap event. XMage plays turn 1 from its untap step, which untaps the
     * active player's permanents, so one the scenario starts tapped is added untapped and
     * tapped here.
     */
    protected void tapAtStart(TestPlayer player, PhaseStep step, String name) {
        runCode("tap " + name + " at the start", 1, step, player, (info, who, game) -> {
            Permanent untapped = controlled(who, name).stream()
                    .filter(p -> !p.isTapped())
                    .findFirst()
                    .orElse(null);
            Assert.assertNotNull(who.getName() + " controls no untapped " + name, untapped);
            untapped.setTapped(true);
        });
    }

    /**
     * Whether `permanent` goes by `name` in a scenario: its own name, or for a face-down
     * permanent, which has none (CR 708.2a), its card's.
     */
    private boolean named(Permanent permanent, String name) {
        if (permanent.getName().equals(name)) {
            return true;
        }
        Card card = currentGame.getCard(permanent.getId());
        return permanent.isFaceDown(currentGame) && card != null && card.getName().equals(name);
    }

    /** The permanents named `name` that `controller` controls, in the order they entered. */
    private List<Permanent> controlled(Player controller, String name) {
        return currentGame.getBattlefield().getAllActivePermanents().stream()
                .filter(p -> p.isControlledBy(controller.getId()) && named(p, name))
                .collect(Collectors.toList());
    }

    private Permanent first(Player controller, String name) {
        List<Permanent> found = controlled(controller, name);
        Assert.assertFalse(controller.getName() + " controls no " + name, found.isEmpty());
        return found.get(0);
    }

    /** How many cards `owner` owns on the battlefield: all of them, or those named `name`. */
    protected void assertOwnedOnBattlefield(Player owner, String name, int count) {
        long found = currentGame.getBattlefield().getAllActivePermanents().stream()
                .filter(p -> p.isOwnedBy(owner.getId()) && (name == null || named(p, name)))
                .count();
        Assert.assertEquals("(Battlefield) " + owner.getName() + " owns " + name, count, found);
    }

    protected void assertTappedControlled(Player controller, String name, int count) {
        long found = controlled(controller, name).stream().filter(Permanent::isTapped).count();
        Assert.assertEquals("tapped " + name + " of " + controller.getName(), count, found);
    }

    protected void assertFaceDownControlled(Player controller, String name, int count) {
        long found = controlled(controller, name).stream().filter(p -> p.isFaceDown(currentGame)).count();
        Assert.assertEquals("face-down " + name + " of " + controller.getName(), count, found);
    }

    protected void assertAttackingControlled(Player controller, String name, int count) {
        long found = controlled(controller, name).stream().filter(Permanent::isAttacking).count();
        Assert.assertEquals("attacking " + name + " of " + controller.getName(), count, found);
    }

    /** The first permanent named `name` that `controller` controls and is a token, or a card. */
    protected void assertPt(Player controller, String name, boolean token, int power, int toughness) {
        Permanent permanent = controlled(controller, name).stream()
                .filter(p -> (p instanceof PermanentToken) == token)
                .findFirst()
                .orElse(null);
        Assert.assertNotNull(controller.getName() + " controls no " + name + (token ? " token" : " card"), permanent);
        Assert.assertEquals("power of " + name, power, permanent.getPower().getValue());
        Assert.assertEquals("toughness of " + name, toughness, permanent.getToughness().getValue());
    }

    protected void assertCreature(Player controller, String name, boolean creature) {
        Assert.assertEquals(name + " is a creature", creature, first(controller, name).isCreature(currentGame));
    }

    /**
     * The first object named `name` in `zone` has exactly the colors `colors`, white first: a
     * permanent `player` controls, a spell they control on the stack, or a card they own anywhere
     * else.
     */
    protected void assertColorsIn(Player player, Zone zone, String name, String... colors) {
        Assert.assertEquals("colors of " + name + " in " + zone, Arrays.asList(colors),
                colorNames(objectIn(player, zone, name).getColor(currentGame)));
    }

    private static List<String> colorNames(ObjectColor color) {
        List<String> names = new ArrayList<>();
        if (color.isWhite()) {
            names.add("White");
        }
        if (color.isBlue()) {
            names.add("Blue");
        }
        if (color.isBlack()) {
            names.add("Black");
        }
        if (color.isRed()) {
            names.add("Red");
        }
        if (color.isGreen()) {
            names.add("Green");
        }
        return names;
    }

    /**
     * The first object named `name` in `zone`, found as by {@link #assertColorsIn}, has this power
     * and toughness: off the battlefield, what a characteristic-defining ability defines there.
     */
    protected void assertPtIn(Player player, Zone zone, String name, int power, int toughness) {
        MageObject object = objectIn(player, zone, name);
        Assert.assertEquals("power of " + name + " in " + zone, power, object.getPower().getValue());
        Assert.assertEquals("toughness of " + name + " in " + zone, toughness, object.getToughness().getValue());
    }

    private MageObject objectIn(Player player, Zone zone, String name) {
        List<? extends MageObject> found;
        switch (zone) {
            case BATTLEFIELD:
                return first(player, name);
            case STACK:
                found = currentGame.getStack().stream()
                        .filter(o -> o instanceof Spell && o.isControlledBy(player.getId()))
                        .collect(Collectors.toList());
                break;
            case HAND:
                found = new ArrayList<>(player.getHand().getCards(currentGame));
                break;
            case GRAVEYARD:
                found = new ArrayList<>(player.getGraveyard().getCards(currentGame));
                break;
            case LIBRARY:
                found = player.getLibrary().getCards(currentGame);
                break;
            default:
                found = currentGame.getExile().getAllCards(currentGame).stream()
                        .filter(c -> c.isOwnedBy(player.getId()))
                        .collect(Collectors.toList());
        }
        return found.stream()
                .filter(o -> o.getName().equals(name))
                .findFirst()
                .orElseThrow(() -> new AssertionError(player.getName() + " has no " + name + " in " + zone));
    }

    protected MageObject snapshotObject(Player player, Zone zone, String name, int index) {
        Stream<? extends MageObject> objects = zone == Zone.BATTLEFIELD
                ? currentGame.getBattlefield().getAllActivePermanents(player.getId()).stream()
                        .filter(p -> name == null ? p.isFaceDown(currentGame)
                                : !p.isFaceDown(currentGame) && p.getName().equals(name))
                : player.getGraveyard().getCards(currentGame).stream().filter(card -> card.getName().equals(name));
        return objects.skip(index).findFirst().orElseThrow(() -> new AssertionError(
                player.getName() + " has no " + (name == null ? "face-down permanent" : name)
                        + " in " + zone + " at index " + index));
    }

    // Beryl's keyword flags have these XMage ability classes. Other abilities are outside
    // the snapshot's vocabulary, including the intrinsic mana abilities of basic land types.
    private static final Map<String, Class<? extends Ability>> SNAPSHOT_KEYWORDS = new HashMap<>();
    static {
        SNAPSHOT_KEYWORDS.put("FIRST_STRIKE", FirstStrikeAbility.class);
        SNAPSHOT_KEYWORDS.put("FLYING", FlyingAbility.class);
        SNAPSHOT_KEYWORDS.put("REACH", ReachAbility.class);
        SNAPSHOT_KEYWORDS.put("VIGILANCE", VigilanceAbility.class);
        SNAPSHOT_KEYWORDS.put("FLASH", FlashAbility.class);
        SNAPSHOT_KEYWORDS.put("HASTE", HasteAbility.class);
        SNAPSHOT_KEYWORDS.put("DELVE", DelveAbility.class);
        SNAPSHOT_KEYWORDS.put("TRAMPLE", TrampleAbility.class);
        SNAPSHOT_KEYWORDS.put("LIFELINK", LifelinkAbility.class);
        SNAPSHOT_KEYWORDS.put("INDESTRUCTIBLE", IndestructibleAbility.class);
        SNAPSHOT_KEYWORDS.put("CHANGELING", ChangelingAbility.class);
        SNAPSHOT_KEYWORDS.put("PLAINSWALK", PlainswalkAbility.class);
        SNAPSHOT_KEYWORDS.put("ISLANDWALK", IslandwalkAbility.class);
        SNAPSHOT_KEYWORDS.put("SWAMPWALK", SwampwalkAbility.class);
        SNAPSHOT_KEYWORDS.put("MOUNTAINWALK", MountainwalkAbility.class);
        SNAPSHOT_KEYWORDS.put("FORESTWALK", ForestwalkAbility.class);
    }

    private static Set<String> snapshotNames(String names) {
        return names.isEmpty() ? Collections.emptySet() : new HashSet<>(Arrays.asList(names.split("[|]")));
    }

    protected void assertPermanentState(Permanent permanent, String colors, String types,
            String subtypes, String keywords, boolean allCreatureTypes, int damage,
            MageObject host, Integer power, Integer toughness) {
        String name = permanent.getName();
        Assert.assertEquals("colors of " + name, snapshotNames(colors),
                new HashSet<>(colorNames(permanent.getColor(currentGame))));
        Assert.assertEquals("types of " + name, snapshotNames(types),
                permanent.getCardType(currentGame).stream().map(Enum::name).collect(Collectors.toSet()));
        Assert.assertEquals("has every creature type: " + name, allCreatureTypes,
                permanent.isAllCreatureTypes(currentGame));
        Assert.assertEquals("subtypes of " + name, snapshotNames(subtypes),
                permanent.getSubtype(currentGame).stream()
                        .filter(subtype -> !allCreatureTypes || !SubType.getCreatureTypes().contains(subtype))
                        .map(SubType::toString).collect(Collectors.toSet()));
        Set<String> actualKeywords = SNAPSHOT_KEYWORDS.entrySet().stream()
                .filter(entry -> permanent.getAbilities(currentGame).stream().anyMatch(entry.getValue()::isInstance))
                .map(Map.Entry::getKey).collect(Collectors.toSet());
        Assert.assertEquals("keywords of " + name, snapshotNames(keywords), actualKeywords);
        Assert.assertEquals("damage marked on " + name, damage, permanent.getDamage());
        Assert.assertEquals("attachment on " + name, host == null ? null : host.getId(), permanent.getAttachedTo());
        if (power != null) {
            Assert.assertEquals("power of " + name, power.intValue(), permanent.getPower().getValue());
            Assert.assertEquals("toughness of " + name, toughness.intValue(), permanent.getToughness().getValue());
        }
    }

    protected void assertStackObject(int depth, Player controller, String name, boolean spell) {
        StackObject item = currentGame.getStack().stream().skip(depth).findFirst()
                .orElseThrow(() -> new AssertionError("empty stack at depth " + depth + ": expected " + name));
        Assert.assertEquals("is a spell at stack depth " + depth, spell, item instanceof Spell);
        Assert.assertTrue("controller of " + name + " on the stack", item.isControlledBy(controller.getId()));
        MageObject source = item instanceof Spell ? item : item.getStackAbility().getSourceObject(currentGame);
        Assert.assertEquals("name at stack depth " + depth, name, source == null ? null : source.getName());
    }

    protected void assertPool(Player player, int... amounts) {
        ManaType[] colors = {ManaType.WHITE, ManaType.BLUE, ManaType.BLACK, ManaType.RED,
                ManaType.GREEN, ManaType.COLORLESS};
        for (int i = 0; i < colors.length; i++) {
            Assert.assertEquals(player.getName() + " mana pool: " + colors[i], amounts[i],
                    player.getManaPool().get(colors[i]));
        }
    }

    protected void assertSubtype(Player controller, String name, SubType subtype, boolean has) {
        Assert.assertFalse(controller.getName() + " controls no " + name, controlled(controller, name).isEmpty());
        boolean found = controlled(controller, name).stream().anyMatch(p -> p.hasSubtype(subtype, currentGame));
        Assert.assertEquals(name + " has subtype " + subtype, has, found);
    }

    protected void assertCountersOn(Player controller, String name, CounterType type, int count) {
        int found = first(controller, name).getCounters(currentGame).getCount(type);
        Assert.assertEquals(type.getName() + " counters on " + name, count, found);
    }

    protected void assertStackSize(int count) {
        Assert.assertEquals("stack size", count, currentGame.getStack().size());
    }

    /** The top cards of `player`'s library, top first. */
    protected void assertLibraryTop(Player player, String... top) {
        List<String> found = player.getLibrary().getTopCards(currentGame, top.length).stream()
                .map(Card::getName)
                .collect(Collectors.toList());
        Assert.assertEquals("top of " + player.getName() + "'s library", Arrays.asList(top), found);
    }
}
