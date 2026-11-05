package org.mage.test.beryl;

import java.io.Serializable;
import java.util.Arrays;
import java.util.List;
import java.util.Map;
import java.util.function.BooleanSupplier;
import java.util.stream.Collectors;

import mage.abilities.Ability;
import mage.cards.Card;
import mage.cards.Cards;
import mage.constants.Outcome;
import mage.constants.PhaseStep;
import mage.constants.RangeOfInfluence;
import mage.counters.CounterType;
import mage.game.Game;
import mage.game.permanent.Permanent;
import mage.game.permanent.PermanentToken;
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

    /**
     * A test player that also takes `passPriority`, `resolveTop` and `setOptionalChoice`. XMage runs a command
     * as soon as its player holds priority, so a command Beryl made after the other player's
     * spell resolved would otherwise run before that spell was cast. An optional choice answers a
     * question Beryl answered without asking, which XMage may or may not ask: it is used if the
     * question comes, and dropped at its player's first priority after the step where Beryl
     * answered it if not.
     */
    private static final class BerylTestPlayer extends TestPlayer {

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
         * Asks `question` with an optional choice at the head of the queue made an ordinary one,
         * and makes it optional again if the question did not use it, as when XMage settles a
         * choice of one card itself. An optional choice reads
         * `beryl:optional:<turn>:<step>:<choice>`, with the turn and step where Beryl answered it.
         */
        private boolean withOptional(BooleanSupplier question) {
            List<String> choices = getChoices();
            if (choices.isEmpty() || !choices.get(0).startsWith(OPTIONAL)) {
                return question.getAsBoolean();
            }
            String optional = choices.get(0);
            int size = choices.size();
            choices.set(0, optional.split(":", 5)[4]);
            boolean result = question.getAsBoolean();
            if (choices.size() == size) {
                choices.set(0, optional);
            }
            return result;
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

        @Override
        public boolean choose(Outcome outcome, Target target, Ability source, Game game, Map<String, Serializable> options) {
            return withOptional(() -> super.choose(outcome, target, source, game, options));
        }

        @Override
        public boolean choose(Outcome outcome, Cards cards, TargetCard target, Ability source, Game game) {
            return withOptional(() -> super.choose(outcome, cards, target, source, game));
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

    /** `choice` answers `player`'s next question, if it comes by the end of `step`. */
    protected void setOptionalChoice(TestPlayer player, int turn, PhaseStep step, String choice) {
        player.addChoice(OPTIONAL + turn + ":" + step.name() + ":" + choice);
    }

    /** The permanents named `name` that `controller` controls, in the order they entered. */
    private List<Permanent> controlled(Player controller, String name) {
        return currentGame.getBattlefield().getAllActivePermanents().stream()
                .filter(p -> p.isControlledBy(controller.getId()) && p.getName().equals(name))
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
                .filter(p -> p.isOwnedBy(owner.getId()) && (name == null || p.getName().equals(name)))
                .count();
        Assert.assertEquals("(Battlefield) " + owner.getName() + " owns " + name, count, found);
    }

    protected void assertTappedControlled(Player controller, String name, int count) {
        long found = controlled(controller, name).stream().filter(Permanent::isTapped).count();
        Assert.assertEquals("tapped " + name + " of " + controller.getName(), count, found);
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
