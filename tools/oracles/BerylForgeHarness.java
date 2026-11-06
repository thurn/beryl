package beryl;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import forge.GuiDesktop;
import forge.ai.ComputerUtil;
import forge.ai.ComputerUtilCost;
import forge.ai.LobbyPlayerAi;
import forge.ai.PlayerControllerAi;
import forge.deck.Deck;
import forge.game.Game;
import forge.game.GameEntity;
import forge.game.GameObject;
import forge.game.GameRules;
import forge.game.GameStage;
import forge.game.GameState;
import forge.game.GameType;
import forge.game.Match;
import forge.game.card.Card;
import forge.game.card.CardCollection;
import forge.game.card.CardCollectionView;
import forge.game.card.CounterEnumType;
import forge.game.card.CounterType;
import forge.game.combat.Combat;
import forge.game.combat.CombatUtil;
import forge.game.cost.Cost;
import forge.game.cost.CostPartWithList;
import forge.game.phase.PhaseHandler;
import forge.game.phase.PhaseType;
import forge.game.player.DelayedReveal;
import forge.game.player.Player;
import forge.game.player.PlayerActionConfirmMode;
import forge.game.player.PlayerController.BinaryChoiceType;
import forge.game.player.PlayerController;
import forge.game.player.RegisteredPlayer;
import forge.game.replacement.ReplacementEffect;
import forge.game.spellability.OptionalCostValue;
import forge.game.spellability.SpellAbility;
import forge.game.spellability.SpellAbilityStackInstance;
import forge.game.spellability.TargetRestrictions;
import forge.game.trigger.WrappedAbility;
import forge.game.zone.ZoneType;
import forge.gui.GuiBase;
import forge.localinstance.properties.ForgePreferences.FPref;
import forge.model.FModel;
import forge.util.MyRandom;
import forge.util.collect.FCollectionView;
import java.io.IOException;
import java.io.Reader;
import java.io.Writer;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.Set;
import org.apache.commons.lang3.tuple.ImmutablePair;

/**
 * Plays Beryl's scenarios in Forge (docs/engine-architecture.md, section 17.2).
 *
 * <p>`by oracle` writes each scenario in Forge's vocabulary: a `forge.game.GameState` text for the
 * setup and the scenario's steps, with card names, phases, ability selectors and optional costs
 * already mapped. This harness loads the state into a fresh two-player game and runs Forge's own
 * game loop with a scripted controller for both seats, a {@link PlayerControllerAi} whose
 * priority decisions and scripted questions come from the steps. Forge asks some questions Beryl
 * answers itself (a single candidate, mana payment, ordering); those keep the AI's answer and
 * are logged. Asserts are checked at the decision they precede, as Beryl's runner does.
 *
 * <p>Usage: {@code java -cp <forge jar>:<classes> beryl.BerylForgeHarness IN.json OUT.json}, from
 * Forge's forge-gui directory, the assets directory a desktop build reads. With
 * BERYL_FORGE_TRACE set, each result's notes also list every priority and the choice that made
 * it a decision, for debugging a scenario.
 */
public final class BerylForgeHarness {
    private BerylForgeHarness() {}

    public static void main(String[] args) throws Exception {
        Throwable[] failure = new Throwable[1];
        // Forge runs game actions synchronously only on a thread whose name starts with "Game"
        // (ThreadUtil.isGameThread); elsewhere it hands them to its executor.
        Thread thread = new Thread(() -> {
            try {
                run(Path.of(args[0]), Path.of(args[1]));
            } catch (Throwable t) {
                failure[0] = t;
            }
        }, "Game-beryl");
        thread.start();
        thread.join();
        if (failure[0] != null) {
            failure[0].printStackTrace();
            System.exit(2);
        }
        // Forge leaves non-daemon worker threads behind.
        System.exit(0);
    }

    private static void run(Path in, Path out) throws IOException {
        GuiBase.setInterface(new GuiDesktop());
        FModel.initialize(null, preferences -> {
            preferences.setPref(FPref.LOAD_CARD_SCRIPTS_LAZILY, false);
            preferences.setPref(FPref.UI_LANGUAGE, "en-US");
            return null;
        });
        JsonArray scenarios;
        try (Reader reader = Files.newBufferedReader(in)) {
            scenarios = JsonParser.parseReader(reader).getAsJsonArray();
        }
        JsonArray results = new JsonArray();
        for (JsonElement element : scenarios) {
            JsonObject scenario = element.getAsJsonObject();
            results.add(new Run(scenario).play());
        }
        Gson gson = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create();
        try (Writer writer = Files.newBufferedWriter(out)) {
            gson.toJson(results, writer);
        }
    }

    /** Thrown from a decision once every step has been played, to leave Forge's game loop. */
    static final class ScriptDone extends Error {
        ScriptDone() {
            super(null, null, false, false);
        }
    }

    /** Something the scenario needs that its export lacks: the scenario is unsupported. */
    static final class Unsupported extends Error {
        Unsupported(String message) {
            super(message, null, false, false);
        }
    }

    /** A step Forge cannot carry out: the scenario fails with this message. */
    static final class StepFailed extends Error {
        StepFailed(String message) {
            super(message, null, false, false);
        }
    }

    /**
     * Forge's random source, seeded with the scenario's seed, which can be told the next coin
     * flip's result. Forge flips with {@code MyRandom.getRandom().nextBoolean()} right after the
     * flipper calls heads or tails, and MyRandom wraps this source in one that draws each value
     * from {@link #nextInt()}, a boolean from its top bit.
     */
    static final class FlipRandom extends Random {
        private Boolean next;

        FlipRandom(long seed) {
            super(seed);
        }

        void force(boolean result) {
            next = result;
        }

        @Override
        public int nextInt() {
            if (next != null) {
                int value = next ? Integer.MIN_VALUE : 0;
                next = null;
                return value;
            }
            return super.nextInt();
        }
    }

    /** Loads a GameState on the calling thread: applyToGame would hand it to Forge's executor. */
    static final class Setup extends GameState {
        void apply(Game game) {
            applyGameOnThread(game);
        }
    }

    /** One scenario: its game, its script and what happened. */
    static final class Run {
        // A scenario that has not finished after this many steps of Forge's game loop is stuck.
        private static final int MAX_LOOP_STEPS = 20000;

        final JsonObject scenario;
        final List<JsonObject> steps = new ArrayList<>();
        final List<String> failures = new ArrayList<>();
        final List<String> notes = new ArrayList<>();
        Game game;
        int next;

        enum Mode { PLAY, RESOLVE, ADVANCE }

        Mode mode = Mode.PLAY;
        // While resolving: the stack entry that has to leave the stack.
        SpellAbilityStackInstance resolving;
        // While advancing: the turn and phase to reach.
        int advanceTurn;
        PhaseType advancePhase;
        // Whether the game has stopped at a priority decision a step asked for (after a Resolve
        // or an AdvanceTo), where asserts apply even if passing is all the player can do.
        boolean stopped;
        // Beryl's coin flips, in order, and the random source that replays them.
        final java.util.ArrayDeque<JsonObject> flips = new java.util.ArrayDeque<>();
        FlipRandom random;
        // The keyword cost (offspring) of the spell being cast, which Forge asks about as it casts.
        String keywordCost;

        Run(JsonObject scenario) {
            this.scenario = scenario;
            for (JsonElement step : scenario.getAsJsonArray("steps")) {
                steps.add(step.getAsJsonObject());
            }
        }

        JsonObject play() {
            JsonObject result = new JsonObject();
            result.addProperty("card", scenario.get("card").getAsString());
            result.addProperty("name", scenario.get("name").getAsString());
            try {
                game = newGame();
                random = new FlipRandom(scenario.get("seed").getAsLong());
                MyRandom.setRandom(random);
                if (scenario.has("flips")) {
                    for (JsonElement flip : scenario.getAsJsonArray("flips")) {
                        flips.add(flip.getAsJsonObject());
                    }
                }
                Setup setup = new Setup();
                setup.parse(List.of(scenario.get("state").getAsString().split("\n")));
                setup.apply(game);
                PhaseType start = game.getPhaseHandler().getPhase();
                if (start.isAfter(PhaseType.MAIN1) && start.isBefore(PhaseType.MAIN2) && game.getCombat() == null) {
                    // A game set up during combat needs the combat its beginning step creates.
                    game.getPhaseHandler().setCombat(new Combat(game.getPhaseHandler().getPlayerTurn()));
                }
                game.getPhaseHandler().onStackResolved();
                stopped = true;
                PhaseHandler phases = game.getPhaseHandler();
                int loopSteps = 0;
                while (!game.isGameOver()) {
                    phases.mainLoopStep();
                    if (++loopSteps > MAX_LOOP_STEPS) {
                        throw new StepFailed("stuck before step " + next + ": " + describeNext());
                    }
                }
                // The game is over: the remaining steps can only be asserts about the end.
                while (next < steps.size()) {
                    JsonObject step = steps.get(next++);
                    if (!step.get("op").getAsString().equals("assert")) {
                        throw new StepFailed("the game ended before step " + (next - 1) + ": " + step);
                    }
                    // Forge takes a player's cards out of the game when they lose, as CR 800.4a
                    // has only in multiplayer games, so their zones can't be counted.
                    JsonObject assertion = step.getAsJsonObject("assertion");
                    if (assertion.get("assert").getAsString().equals("Count") && player(assertion).hasLost()) {
                        notes.add("unchecked after p" + seat(player(assertion)) + " lost: " + assertion);
                        continue;
                    }
                    check(step);
                }
            } catch (ScriptDone done) {
                // Every step played.
            } catch (Unsupported e) {
                result.addProperty("status", "unsupported");
                result.addProperty("reason", e.getMessage());
                result.add("notes", strings(notes));
                return result;
            } catch (StepFailed e) {
                failures.add(e.getMessage());
            } catch (Throwable t) {
                failures.add("Forge threw " + t);
                notes.add(stackTrace(t));
            }
            result.addProperty("status", failures.isEmpty() ? "pass" : "fail");
            result.add("failures", strings(failures));
            result.add("notes", strings(notes));
            return result;
        }

        private Game newGame() {
            List<RegisteredPlayer> players = new ArrayList<>();
            for (int seat = 0; seat < 2; seat++) {
                players.add(new RegisteredPlayer(new Deck()).setPlayer(new ScriptedLobbyPlayer("p" + seat, this)));
            }
            GameRules rules = new GameRules(GameType.Constructed);
            Match match = new Match(rules, players, "Beryl");
            Game created = new Game(players, rules, match);
            created.setAge(GameStage.Play);
            created.EXPERIMENTAL_RESTORE_SNAPSHOT = false;
            return created;
        }

        // ---- The script ----

        // By the registered players: Forge drops a player who has lost from getPlayers().
        int seat(Player player) {
            return game.getRegisteredPlayers().indexOf(player);
        }

        Player player(JsonObject object) {
            return game.getRegisteredPlayers().get(object.get("player").getAsInt());
        }

        String describeNext() {
            return next < steps.size() ? steps.get(next).toString() : "(end)";
        }

        /** Checks the asserts at the head of the script; throws ScriptDone at its end. */
        void checkAsserts() {
            while (next < steps.size() && op(steps.get(next)).equals("assert")) {
                check(steps.get(next++));
            }
            if (next >= steps.size()) {
                throw new ScriptDone();
            }
        }

        static String op(JsonObject step) {
            return step.get("op").getAsString();
        }

        /** The first step after any asserts at the head of the script, or null at its end. */
        JsonObject peekPastAsserts() {
            for (int i = next; i < steps.size(); i++) {
                if (!op(steps.get(i)).equals("assert")) {
                    return steps.get(i);
                }
            }
            return null;
        }

        boolean isAnswer(JsonObject step, Player player, Set<String> kinds) {
            return step != null
                    && op(step).equals("answer")
                    && step.get("player").getAsInt() == seat(player)
                    && kinds.contains(step.get("kind").getAsString());
        }

        /**
         * At a question Forge asks `player` other than priority: if the script's next step (past
         * asserts) answers it, checks those asserts here, ends a Resolve and returns the pick;
         * otherwise returns null and the AI answers.
         */
        JsonObject take(Player player, Set<String> kinds, String question) {
            if (mode == Mode.ADVANCE || !isAnswer(peekPastAsserts(), player, kinds)) {
                notes.add("unscripted: " + question + " for p" + seat(player) + " (AI answered)");
                return null;
            }
            checkAsserts();
            mode = Mode.PLAY;
            stopped = false;
            return steps.get(next++).getAsJsonObject("pick");
        }

        /** Takes the next answer only if it is one that picks a card among `options`. */
        JsonObject takeFollowingAmong(Player player, Set<String> kinds, List<? extends GameObject> options) {
            if (next >= steps.size() || !isAnswer(steps.get(next), player, kinds)) {
                return null;
            }
            JsonObject pick = steps.get(next).getAsJsonObject("pick");
            if (!pick.has("card")) {
                return null;
            }
            String name = pick.get("card").getAsString();
            for (GameObject option : options) {
                if (option instanceof Card card && named(card, name)) {
                    next++;
                    return pick;
                }
            }
            return null;
        }

        /** Takes the next answer only if it is one, without checking asserts first. */
        JsonObject takeFollowing(Player player, Set<String> kinds) {
            if (next < steps.size() && isAnswer(steps.get(next), player, kinds)) {
                return steps.get(next++).getAsJsonObject("pick");
            }
            return null;
        }

        // ---- Priority ----

        List<SpellAbility> priority(Player player) {
            PhaseHandler phases = game.getPhaseHandler();
            boolean active = phases.getPlayerTurn() == player;
            if (System.getenv("BERYL_FORGE_TRACE") != null) {
                notes.add("priority p" + seat(player) + " " + where() + " mode " + mode + " next " + next);
            }
            if (mode == Mode.ADVANCE) {
                if (!reached()) {
                    return null;
                }
                if (!active) {
                    return null;
                }
                mode = Mode.PLAY;
                stopped = true;
            }
            if (mode == Mode.RESOLVE) {
                if (onStack(resolving) || !active) {
                    return null;
                }
                mode = Mode.PLAY;
                stopped = true;
            }
            // Where Beryl's runner stops, as its exact profile does: at a decision with more than
            // a pass in it, and at a breakpoint the next step sets: the active player's next
            // priority before a Resolve, the target of an AdvanceTo.
            JsonObject head = peekPastAsserts();
            boolean mine = head != null && isPriorityStep(head, player);
            boolean stop;
            if (stopped || (mine && op(head).equals("answer"))) {
                stop = true;
            } else if (head != null && op(head).equals("resolve")) {
                stop = active || hasChoice(player);
            } else if (head != null && op(head).equals("advance")) {
                stop = active && at(head) || hasChoice(player);
            } else {
                stop = hasChoice(player);
            }
            if (!stop) {
                return null;
            }
            checkAsserts();
            JsonObject step = steps.get(next);
            switch (op(step)) {
                case "resolve" -> {
                    next++;
                    stopped = false;
                    if (game.getStack().isEmpty()) {
                        throw new StepFailed("Resolve with an empty stack at step " + (next - 1));
                    }
                    resolving = game.getStack().peek();
                    mode = Mode.RESOLVE;
                    return null;
                }
                case "advance" -> {
                    next++;
                    stopped = false;
                    advanceTurn = step.get("turn").getAsInt();
                    advancePhase = PhaseType.valueOf(step.get("phase").getAsString());
                    mode = Mode.ADVANCE;
                    if (at(step) && active) {
                        mode = Mode.PLAY;
                        stopped = true;
                        return priority(player);
                    }
                    return null;
                }
                default -> {
                }
            }
            if (!mine) {
                if (hasChoice(player)) {
                    notes.add("unscripted: priority for p" + seat(player) + " at " + phases.getPhase()
                            + " before " + step + " (passed)");
                }
                return null;
            }
            next++;
            stopped = false;
            return switch (op(step)) {
                case "answer" -> null; // a priority pass
                case "play_land" -> List.of(landPlay(player, step));
                case "cast" -> List.of(cast(player, step));
                case "activate" -> List.of(activate(player, step));
                default -> throw new StepFailed("unknown step " + step);
            };
        }

        boolean isPriorityStep(JsonObject step, Player player) {
            String op = op(step);
            if (op.equals("answer")) {
                return step.get("kind").getAsString().equals("Priority") && step.get("player").getAsInt() == seat(player);
            }
            return (op.equals("cast") || op.equals("activate") || op.equals("play_land"))
                    && step.get("player").getAsInt() == seat(player);
        }

        boolean at(JsonObject advance) {
            PhaseHandler phases = game.getPhaseHandler();
            return phases.getTurn() == advance.get("turn").getAsInt()
                    && phases.getPhase() == PhaseType.valueOf(advance.get("phase").getAsString());
        }

        boolean reached() {
            PhaseHandler phases = game.getPhaseHandler();
            int turn = phases.getTurn();
            if (turn > advanceTurn || (turn == advanceTurn && phases.getPhase().isAfter(advancePhase))) {
                throw new StepFailed("passed turn " + advanceTurn + " " + advancePhase + " without priority there");
            }
            return turn == advanceTurn && phases.getPhase() == advancePhase;
        }

        boolean onStack(SpellAbilityStackInstance entry) {
            for (SpellAbilityStackInstance si : game.getStack()) {
                if (si == entry) {
                    return true;
                }
            }
            return false;
        }

        /** Whether `player` could do anything but pass: play a land, cast a spell, activate a non-mana ability. */
        boolean hasChoice(Player player) {
            for (Card card : game.getCardsInGame()) {
                if (card.isInZone(ZoneType.Stack) || card.isInZone(ZoneType.Library)) {
                    continue;
                }
                for (SpellAbility sa : card.getAllPossibleAbilities(player, true)) {
                    if (sa.isManaAbility()) {
                        continue;
                    }
                    if (sa.isLandAbility()) {
                        if (player.canPlayLand(card, false, sa)) {
                            return true;
                        }
                        continue;
                    }
                    if (sa.canPlay() && ComputerUtilCost.canPayCost(sa, player, false) && hasTargets(sa)) {
                        if (System.getenv("BERYL_FORGE_TRACE") != null) {
                            notes.add("choice for p" + seat(player) + ": " + sa);
                        }
                        return true;
                    }
                }
            }
            return false;
        }

        boolean hasTargets(SpellAbility sa) {
            for (SpellAbility part = sa; part != null; part = part.getSubAbility()) {
                if (part.usesTargeting()) {
                    TargetRestrictions tr = part.getTargetRestrictions();
                    if (tr.getMinTargets(part.getHostCard(), part) > 0 && candidates(part).isEmpty()) {
                        return false;
                    }
                }
            }
            return true;
        }

        List<GameObject> candidates(SpellAbility sa) {
            List<GameObject> result = new ArrayList<>();
            for (GameEntity entity : sa.getTargetRestrictions().getAllCandidates(sa, false)) {
                // A spell is targeted as itself, not as its card on the stack.
                if (!(entity instanceof Card card && card.isInZone(ZoneType.Stack))) {
                    result.add(entity);
                }
            }
            for (SpellAbilityStackInstance si : game.getStack()) {
                if (sa.canTargetSpellAbility(si.getSpellAbility())) {
                    result.add(si.getSpellAbility());
                }
            }
            return result;
        }

        // ---- Actions ----

        Card find(Player player, String name, ZoneType... zones) {
            for (ZoneType zone : zones) {
                for (Card card : game.getCardsIn(zone)) {
                    if (named(card, name) && (zone == ZoneType.Battlefield ? card.getController() : card.getOwner()) == player) {
                        return card;
                    }
                }
            }
            throw new StepFailed("p" + seat(player) + " has no " + name);
        }

        SpellAbility landPlay(Player player, JsonObject step) {
            Card card = find(player, step.get("card").getAsString(), ZoneType.Hand);
            for (SpellAbility sa : card.getAllPossibleAbilities(player, true)) {
                if (sa.isLandAbility()) {
                    return sa;
                }
            }
            throw new StepFailed("Forge offers no land play for " + card);
        }

        SpellAbility cast(Player player, JsonObject step) {
            String name = step.get("card").getAsString();
            String cost = step.has("cost") ? step.get("cost").getAsString() : null;
            Card card = find(player, name, ZoneType.Hand, ZoneType.Graveyard, ZoneType.Exile, ZoneType.Library);
            SpellAbility chosen = null;
            SpellAbility plain = null;
            for (SpellAbility sa : card.getAllPossibleAbilities(player, true)) {
                if (!sa.isSpell()) {
                    continue;
                }
                boolean alternative = sa.getAlternativeCost() != null;
                if (!alternative && plain == null) {
                    plain = sa;
                }
                if (alternative && cost != null && sa.getAlternativeCost().name().equalsIgnoreCase(cost)) {
                    chosen = sa;
                }
            }
            if (chosen == null) {
                chosen = plain; // with an optional cost, chosen on the spell itself below
            }
            if (chosen == null) {
                throw new StepFailed("Forge offers no castable spell for " + name + (cost == null ? "" : " with " + cost));
            }
            if (cost != null && chosen.getAlternativeCost() == null) {
                List<OptionalCostValue> picked = new ArrayList<>();
                for (OptionalCostValue value : forge.game.GameActionUtil.getOptionalCostValues(chosen)) {
                    if (value.getType().name().equalsIgnoreCase(cost) || value.toString().contains(cost)) {
                        picked.add(value);
                    }
                }
                if (picked.isEmpty()) {
                    keywordCost = cost; // a keyword cost, chosen as Forge casts
                } else {
                    chosen = forge.game.GameActionUtil.addOptionalCosts(chosen, picked);
                }
            }
            chosen.setActivatingPlayer(player);
            prepare(player, chosen);
            return chosen;
        }

        SpellAbility activate(Player player, JsonObject step) {
            String name = step.get("card").getAsString();
            String selector = step.get("ability").getAsString();
            Card card = find(player, name, ZoneType.Battlefield, ZoneType.Hand, ZoneType.Graveyard);
            List<String> seen = new ArrayList<>();
            for (SpellAbility sa : card.getAllPossibleAbilities(player, true)) {
                if (sa.isSpell() || sa.isLandAbility() || sa.isManaAbility()) {
                    continue;
                }
                seen.add(sa.toString());
                if (sa.toString().contains(selector)) {
                    sa.setActivatingPlayer(player);
                    prepare(player, sa);
                    return sa;
                }
            }
            throw new StepFailed("Forge offers no ability of " + name + " matching " + selector + "; it offers " + seen);
        }

        /** Takes the casting's own answers from the script (X, how many targets, the targets) and applies them. */
        void prepare(Player player, SpellAbility sa) {
            JsonObject x = takeFollowing(player, Set.of("X"));
            if (x != null) {
                sa.setXManaCostPaid(x.get("n").getAsInt());
            }
            chooseTargets(player, sa);
        }

        /** Chooses each targeted part's targets from the script's TargetCount and Target answers, or the only candidates. */
        void chooseTargets(Player player, SpellAbility sa) {
            for (SpellAbility part = sa; part != null; part = part.getSubAbility()) {
                if (!part.usesTargeting()) {
                    continue;
                }
                part.resetTargets();
                TargetRestrictions tr = part.getTargetRestrictions();
                int min = tr.getMinTargets(part.getHostCard(), part);
                int max = tr.getMaxTargets(part.getHostCard(), part);
                JsonObject count = takeFollowing(player, Set.of("TargetCount"));
                int wanted = count != null ? count.get("n").getAsInt() : min;
                List<GameObject> candidates = candidates(part);
                for (int i = 0; i < wanted; i++) {
                    JsonObject pick = takeFollowing(player, Set.of("Target"));
                    GameObject target;
                    if (pick != null) {
                        target = resolvePick(pick, candidates);
                    } else if (candidates.size() == 1 || (max == 1 && wanted == 1 && sameName(candidates))) {
                        // Beryl asks nothing of one candidate, or of interchangeable ones.
                        target = candidates.get(0);
                    } else {
                        throw new StepFailed("unscripted target for " + part + " among " + candidates);
                    }
                    candidates.remove(target);
                    part.getTargets().add(target);
                }
            }
        }

        static boolean sameName(List<GameObject> objects) {
            if (objects.isEmpty()) {
                return false;
            }
            String first = null;
            for (GameObject object : objects) {
                if (!(object instanceof Card card)) {
                    return false;
                }
                if (first == null) {
                    first = card.getName();
                } else if (!first.equals(card.getName())) {
                    return false;
                }
            }
            return true;
        }

        GameObject resolvePick(JsonObject pick, List<? extends GameObject> options) {
            String kind = pick.get("pick").getAsString();
            for (GameObject option : options) {
                switch (kind) {
                    case "Card" -> {
                        String name = pick.get("card").getAsString();
                        if (option instanceof Card card && named(card, name)) {
                            return option;
                        }
                        if (option instanceof SpellAbility spell && named(spell.getHostCard(), name)) {
                            return option;
                        }
                    }
                    case "Player" -> {
                        if (option instanceof Player p && seat(p) == pick.get("player").getAsInt()) {
                            return option;
                        }
                    }
                    default -> throw new StepFailed("a " + kind + " pick where Forge wants one of " + options);
                }
            }
            throw new StepFailed("no " + pick + " among " + options);
        }

        // ---- Asserts ----

        static boolean named(Card card, String name) {
            return card.getName().equals(name);
        }

        List<Card> controlled(Player player, String name) {
            List<Card> result = new ArrayList<>();
            for (Card card : player.getCardsIn(ZoneType.Battlefield)) {
                if (named(card, name)) {
                    result.add(card);
                }
            }
            return result;
        }

        Card first(Player player, String name, Boolean token) {
            for (Card card : controlled(player, name)) {
                if (token == null || card.isToken() == token) {
                    return card;
                }
            }
            throw new StepFailed("p" + seat(player) + " controls no " + (Boolean.TRUE.equals(token) ? "token " : "") + name);
        }

        void check(JsonObject step) {
            JsonObject a = step.getAsJsonObject("assertion");
            String kind = a.get("assert").getAsString();
            Object expected;
            Object actual;
            try {
                switch (kind) {
                    case "Life" -> {
                        expected = a.get("life").getAsInt();
                        actual = player(a).getLife();
                    }
                    case "Count" -> {
                        expected = a.get("count").getAsInt();
                        ZoneType zone = ZoneType.valueOf(a.get("zone").getAsString());
                        String name = a.get("card").isJsonNull() ? null : a.get("card").getAsString();
                        int count = 0;
                        for (Card card : game.getCardsIn(zone)) {
                            if (card.getOwner() == player(a) && (name == null || named(card, name))) {
                                count++;
                            }
                        }
                        actual = count;
                    }
                    case "Tapped" -> {
                        expected = a.get("count").getAsInt();
                        int count = 0;
                        for (Card card : controlled(player(a), a.get("card").getAsString())) {
                            if (card.isTapped()) {
                                count++;
                            }
                        }
                        actual = count;
                    }
                    case "Controls" -> {
                        expected = a.get("count").getAsInt();
                        actual = controlled(player(a), a.get("card").getAsString()).size();
                    }
                    case "Attacking" -> {
                        expected = a.get("count").getAsInt();
                        Combat combat = game.getCombat();
                        int count = 0;
                        for (Card card : controlled(player(a), a.get("card").getAsString())) {
                            if (combat != null && combat.isAttacking(card)) {
                                count++;
                            }
                        }
                        actual = count;
                    }
                    case "Pt" -> {
                        Card card = first(player(a), a.get("card").getAsString(), a.get("token").getAsBoolean());
                        expected = a.get("power").getAsInt() + "/" + a.get("toughness").getAsInt();
                        actual = card.getNetPower() + "/" + card.getNetToughness();
                    }
                    case "Creature" -> {
                        expected = a.get("creature").getAsBoolean();
                        actual = first(player(a), a.get("card").getAsString(), null).isCreature();
                    }
                    case "Counters" -> {
                        expected = a.get("count").getAsInt();
                        CounterType type = CounterEnumType.valueOf(a.get("kind").getAsString());
                        actual = first(player(a), a.get("card").getAsString(), null).getCounters(type);
                    }
                    case "Stack" -> {
                        expected = a.get("count").getAsInt();
                        actual = game.getStack().size();
                    }
                    case "Library" -> {
                        List<String> top = new ArrayList<>();
                        for (JsonElement name : a.getAsJsonArray("top")) {
                            top.add(name.getAsString());
                        }
                        expected = top;
                        List<String> names = new ArrayList<>();
                        CardCollectionView library = player(a).getCardsIn(ZoneType.Library);
                        for (int i = 0; i < Math.min(top.size(), library.size()); i++) {
                            names.add(library.get(i).getName());
                        }
                        actual = names;
                    }
                    case "Mana" -> {
                        expected = a.get("amount").getAsInt();
                        byte color = (byte) forge.card.mana.ManaAtom.class.getField(a.get("color").getAsString()).getInt(null);
                        actual = player(a).getManaPool().getAmountOfColor(color);
                    }
                    case "Outcome" -> {
                        JsonObject outcome = a.getAsJsonObject("outcome");
                        expected = outcome.toString();
                        JsonObject now = new JsonObject();
                        if (!game.isGameOver()) {
                            now.addProperty("ending", "InProgress");
                        } else if (game.getOutcome().isDraw()) {
                            now.addProperty("ending", "Draw");
                        } else {
                            now.addProperty("ending", "Win");
                            now.addProperty("player", seat(game.getOutcome().getWinningPlayer().getPlayer() == null
                                    ? null : winner()));
                        }
                        actual = now.toString();
                    }
                    default -> throw new StepFailed("unknown assert " + kind);
                }
            } catch (StepFailed e) {
                failures.add("step " + (next - 1) + " " + a + ": " + e.getMessage());
                return;
            } catch (ReflectiveOperationException e) {
                throw new StepFailed("bad mana color in " + a);
            }
            if (!expected.equals(actual)) {
                failures.add("step " + (next - 1) + " " + a + ": Forge has " + actual + " at " + where());
            }
        }

        Player winner() {
            for (Player p : game.getRegisteredPlayers()) {
                if (!p.hasLost()) {
                    return p;
                }
            }
            return null;
        }

        String where() {
            PhaseHandler phases = game.getPhaseHandler();
            return "turn " + phases.getTurn() + " " + phases.getPhase() + ", stack " + game.getStack().size();
        }

        static JsonArray strings(List<String> values) {
            JsonArray array = new JsonArray();
            values.forEach(array::add);
            return array;
        }

        static String stackTrace(Throwable t) {
            java.io.StringWriter writer = new java.io.StringWriter();
            t.printStackTrace(new java.io.PrintWriter(writer));
            String text = writer.toString();
            return text.length() > 4000 ? text.substring(0, 4000) : text;
        }
    }

    static final class ScriptedLobbyPlayer extends LobbyPlayerAi {
        private final Run run;

        ScriptedLobbyPlayer(String name, Run run) {
            super(name, null);
            this.run = run;
        }

        @Override
        public Player createIngamePlayer(Game game, int id) {
            Player player = new Player(getName(), game, id);
            player.setFirstController(new ScriptedController(game, player, this, run));
            return player;
        }

        @Override
        public PlayerController createMindSlaveController(Player master, Player slave) {
            return new ScriptedController(slave.getGame(), slave, this, run);
        }
    }

    /** The AI's controller, with the script's answers where it has them. */
    static final class ScriptedController extends PlayerControllerAi {
        private static final Set<String> YES_NO = Set.of("YesNo", "PayLife");
        private static final Set<String> OBJECTS = Set.of("ChooseObject", "ChooseUpTo", "Target", "Discard", "AttackTarget", "CastOffer");
        private static final Set<String> OPTIONAL_OBJECTS = Set.of("ChooseObject", "ChooseUpTo", "Target", "Discard", "AttackTarget", "CastOffer", "YesNo");

        private final Run run;
        // What the script picked since this player's last priority, in its order.
        private final List<Object> lastPicks = new ArrayList<>();

        ScriptedController(Game game, Player player, LobbyPlayerAi lobby, Run run) {
            super(game, player, lobby);
            this.run = run;
        }

        @Override
        public List<SpellAbility> chooseSpellAbilityToPlay() {
            lastPicks.clear();
            return run.priority(player);
        }

        @Override
        public boolean playChosenSpellAbility(SpellAbility sa) {
            if (sa.isLandAbility()) {
                if (!sa.canPlay()) {
                    throw new StepFailed("Forge does not allow " + sa);
                }
                sa.resolve();
                return true;
            }
            int before = getGame().getStack().size();
            // Targets are already chosen; payment is the AI's.
            boolean played = ComputerUtil.handlePlayingSpellAbility(player, sa, null);
            if (!played) {
                throw new StepFailed("Forge did not play " + sa + " (stack " + before + ")");
            }
            return true;
        }

        boolean yes(Set<String> kinds, String question, boolean otherwise) {
            JsonObject pick = run.take(player, kinds, question);
            if (pick == null) {
                return otherwise;
            }
            return switch (pick.get("pick").getAsString()) {
                case "Yes" -> true;
                case "No", "Done" -> false;
                default -> throw new StepFailed("a " + pick + " answer to " + question);
            };
        }

        @Override
        public boolean chooseBinary(SpellAbility sa, String question, BinaryChoiceType kind, Boolean defaultVal) {
            if (kind == BinaryChoiceType.HeadsOrTails) {
                // Replays Beryl's flip (CR 705.2): call heads, and the coin lands as Beryl's did.
                if (!run.scenario.has("flips")) {
                    throw new Unsupported("a coin flip, and the dump has no trace of Beryl's flips");
                }
                JsonObject flip = run.flips.poll();
                if (flip == null) {
                    throw new StepFailed("p" + run.seat(player) + " flips a coin Beryl did not flip");
                } else {
                    if (flip.get("player").getAsInt() != run.seat(player)) {
                        throw new StepFailed("p" + run.seat(player) + " flips a coin where Beryl's p" + flip.get("player") + " did");
                    }
                    run.random.force(flip.get("won").getAsBoolean());
                }
                return true;
            }
            return super.chooseBinary(sa, question, kind, defaultVal);
        }

        @Override
        public boolean confirmAction(SpellAbility sa, PlayerActionConfirmMode mode, String message, List<String> options, Card cardToShow, Map<String, Object> params) {
            return yes(YES_NO, "confirm " + message, super.confirmAction(sa, mode, message, options, cardToShow, params));
        }

        @Override
        public boolean confirmTrigger(WrappedAbility wrapper) {
            if (wrapper.isMandatory()) {
                return true;
            }
            return yes(YES_NO, "optional trigger " + wrapper, true);
        }

        @Override
        public boolean confirmReplacementEffect(ReplacementEffect re, SpellAbility effectSA, GameEntity affected, String question) {
            return yes(YES_NO, "replacement " + question, super.confirmReplacementEffect(re, effectSA, affected, question));
        }

        @Override
        public boolean payCostToPreventEffect(Cost cost, SpellAbility sa, boolean alreadyPaid, FCollectionView<Player> allPayers) {
            if (!ComputerUtilCost.canPayCost(cost, sa, player, true)) {
                return false;
            }
            JsonObject pick = run.take(player, YES_NO, "pay " + cost + " for " + sa);
            if (pick == null) {
                return super.payCostToPreventEffect(cost, sa, alreadyPaid, allPayers);
            }
            if (!pick.get("pick").getAsString().equals("Yes")) {
                return false;
            }
            forge.game.cost.CostPayment pay = new forge.game.cost.CostPayment(cost, sa);
            return pay.payComputerCosts(new forge.ai.AiCostDecision(player, sa, true));
        }

        @Override
        public <T extends GameEntity> T chooseSingleEntityForEffect(FCollectionView<T> options, DelayedReveal delayedReveal, SpellAbility sa, String title, boolean isOptional, Player targetedPlayer, Map<String, Object> params) {
            if (options.size() == 1 && !isOptional) {
                return options.get(0);
            }
            List<T> picked = pickMany(List.copyOf(options), isOptional ? 0 : 1, 1, "choose " + title);
            if (picked == null) {
                return super.chooseSingleEntityForEffect(options, delayedReveal, sa, title, isOptional, targetedPlayer, params);
            }
            return picked.isEmpty() ? null : picked.get(0);
        }

        @Override
        public <T extends GameEntity> List<T> chooseEntitiesForEffect(FCollectionView<T> options, int min, int max, DelayedReveal delayedReveal, SpellAbility sa, String title, Player relatedPlayer, Map<String, Object> params) {
            List<T> picked = pickMany(List.copyOf(options), min, max, "choose " + title);
            return picked != null ? picked : super.chooseEntitiesForEffect(options, min, max, delayedReveal, sa, title, relatedPlayer, params);
        }

        @Override
        public CardCollectionView chooseCardsForEffect(CardCollectionView options, SpellAbility sa, String title, int min, int max, boolean isOptional, Map<String, Object> params) {
            List<Card> picked = pickMany(List.copyOf(options), isOptional ? 0 : min, max, "choose " + title);
            return picked != null ? new CardCollection(picked) : super.chooseCardsForEffect(options, sa, title, min, max, isOptional, params);
        }

        @Override
        public CardCollection chooseCardsToDiscardFrom(Player p, SpellAbility sa, CardCollection valid, int min, int max, CardCollectionView visible) {
            List<Card> picked = pickMany(List.copyOf(valid), min, max, "discard");
            return picked != null ? new CardCollection(picked) : super.chooseCardsToDiscardFrom(p, sa, valid, min, max, visible);
        }

        @Override
        public CardCollectionView chooseCardsForCost(CardCollectionView options, SpellAbility sa, CostPartWithList cpl, int amount, boolean isOptional, String prompt) {
            List<Card> picked = pickMany(List.copyOf(options), isOptional ? 0 : amount, amount, "cost " + prompt);
            return picked != null ? new CardCollection(picked) : super.chooseCardsForCost(options, sa, cpl, amount, isOptional, prompt);
        }

        /** Card picks until Done or `max`, or null if the script has none here. */
        <T> List<T> pickMany(List<T> options, int min, int max, String question) {
            if (options.size() == min && min == max) {
                lastPicks.addAll(options);
                return new ArrayList<>(options);
            }
            JsonObject pick = run.take(player, min == 0 ? OPTIONAL_OBJECTS : OBJECTS, question + " from " + options);
            if (pick == null) {
                return null;
            }
            List<T> remaining = new ArrayList<>(options);
            List<T> picked = new ArrayList<>();
            // Beryl asks whether to take an optional pick, then which, unless only one can be taken.
            if (pick.get("pick").getAsString().equals("No")) {
                return picked;
            }
            if (pick.get("pick").getAsString().equals("Yes")) {
                if (options.size() == 1) {
                    picked.add(options.get(0));
                    return picked;
                }
                pick = run.takeFollowing(player, OBJECTS);
                if (pick == null) {
                    throw new StepFailed("a Yes to " + question + " without a pick among " + options);
                }
            }
            while (pick != null && !pick.get("pick").getAsString().equals("Done")) {
                @SuppressWarnings("unchecked")
                T chosen = (T) run.resolvePick(pick, (List<? extends GameObject>) remaining);
                remaining.remove(chosen);
                picked.add(chosen);
                lastPicks.add(chosen);
                if (picked.size() >= max) {
                    break;
                }
                pick = run.takeFollowing(player, OBJECTS);
            }
            return picked;
        }

        @Override
        public CardCollectionView orderMoveToZoneList(CardCollectionView cards, ZoneType destination, SpellAbility source) {
            // Cards the script picked one at a time move in its order, each onto the last.
            if (!cards.isEmpty() && lastPicks.containsAll(cards)) {
                return cards;
            }
            // Cards for the bottom of a library that the script orders next, as Stock Up's rest:
            // Beryl puts each under the one before, the last unasked, as Forge moves them in turn.
            if (destination == ZoneType.Library && cards.size() > 1) {
                List<Card> rest = new ArrayList<>(cards);
                CardCollection order = new CardCollection();
                while (rest.size() > 1) {
                    JsonObject pick = run.takeFollowingAmong(player, Set.of("ChooseObject"), rest);
                    if (pick == null) {
                        break;
                    }
                    Card card = (Card) run.resolvePick(pick, rest);
                    rest.remove(card);
                    order.add(card);
                }
                if (!order.isEmpty()) {
                    order.addAll(rest);
                    return order;
                }
            }
            return super.orderMoveToZoneList(cards, destination, source);
        }

        @Override
        public CardCollectionView chooseCardsToDiscardUnlessType(int num, CardCollectionView hand, String[] uTypes, SpellAbility sa) {
            // Beryl asks whether to discard a card of the type instead when that differs from
            // discarding `num` (a hand of more than one card), then which; else the `num` discards.
            List<Card> ofType = new ArrayList<>();
            for (Card card : hand) {
                if (card.isValid(uTypes, sa.getActivatingPlayer(), sa.getHostCard(), sa)) {
                    ofType.add(card);
                }
            }
            boolean instead = !ofType.isEmpty()
                    && (hand.size() < 2 || yes(YES_NO, "discard " + String.join(",", uTypes) + " instead", false));
            List<Card> options = instead ? ofType : List.copyOf(hand);
            int count = instead ? 1 : Math.min(num, hand.size());
            List<Card> picked = pickMany(options, count, count, "discard");
            return picked != null ? new CardCollection(picked) : super.chooseCardsToDiscardUnlessType(num, hand, uTypes, sa);
        }

        @Override
        public ImmutablePair<CardCollection, CardCollection> arrangeForScry(CardCollection topN) {
            List<Card> bottom = pickMany(List.copyOf(topN), 0, topN.size(), "scry (to the bottom)");
            if (bottom == null) {
                return super.arrangeForScry(topN);
            }
            // Then the order of those left on top, topmost first (Forge's order for its left list).
            List<Card> rest = new ArrayList<>(topN);
            rest.removeAll(bottom);
            CardCollection top = new CardCollection();
            while (rest.size() > 1) {
                JsonObject pick = run.takeFollowing(player, Set.of("ChooseObject"));
                if (pick == null) {
                    break;
                }
                Card card = (Card) run.resolvePick(pick, rest);
                rest.remove(card);
                top.add(card);
            }
            top.addAll(rest);
            return ImmutablePair.of(top, new CardCollection(bottom));
        }

        @Override
        public void declareAttackers(Player attacker, Combat combat) {
            JsonObject pick = run.take(player, Set.of("Attacker"), "declare attackers");
            if (pick == null) {
                return; // no attack unless the script says so
            }
            while (pick != null && pick.get("pick").getAsString().equals("Card")) {
                Card card = run.first(player, pick.get("card").getAsString(), null);
                GameEntity defender = attacker.getSingleOpponent();
                JsonObject target = run.takeFollowing(player, Set.of("AttackTarget"));
                if (target != null && target.get("pick").getAsString().equals("Card")) {
                    defender = run.first(defender instanceof Player p ? p : attacker.getSingleOpponent(), target.get("card").getAsString(), null);
                }
                if (!CombatUtil.canAttack(card, defender)) {
                    throw new StepFailed("Forge does not let " + card + " attack " + defender);
                }
                combat.addAttacker(card, defender);
                pick = run.takeFollowing(player, Set.of("Attacker"));
            }
        }

        @Override
        public void declareBlockers(Player defender, Combat combat) {
            JsonObject pick = run.take(player, Set.of("Blocker"), "declare blockers");
            if (pick == null) {
                return; // no block unless the script says so
            }
            while (pick != null && pick.get("pick").getAsString().equals("Card")) {
                Card blocker = run.first(player, pick.get("card").getAsString(), null);
                Card attacker = null;
                for (Card candidate : combat.getAttackers()) {
                    if (CombatUtil.canBlock(candidate, blocker, combat)) {
                        attacker = candidate;
                        break;
                    }
                }
                if (attacker == null) {
                    throw new StepFailed("Forge does not let " + blocker + " block");
                }
                combat.addBlocker(attacker, blocker);
                pick = run.takeFollowing(player, Set.of("Blocker"));
            }
        }

        @Override
        public boolean playTrigger(Card host, WrappedAbility wrapper, boolean isMandatory) {
            boolean scripted = false;
            for (SpellAbility part = wrapper.getWrappedAbility(); part != null; part = part.getSubAbility()) {
                scripted |= part.usesTargeting();
            }
            if (scripted && run.isAnswer(run.peekPastAsserts(), player, Set.of("Target", "TargetCount"))) {
                run.checkAsserts();
                run.mode = Run.Mode.PLAY;
                run.chooseTargets(player, wrapper.getWrappedAbility());
                return ComputerUtil.playNoStack(wrapper.getActivatingPlayer(), wrapper, getGame(), true);
            }
            return super.playTrigger(host, wrapper, isMandatory);
        }

        @Override
        public boolean playSaFromPlayEffect(SpellAbility spell) {
            JsonObject pick = run.take(player, Set.of("CastOffer", "YesNo"), "cast " + spell);
            if (pick == null) {
                return super.playSaFromPlayEffect(spell);
            }
            String kind = pick.get("pick").getAsString();
            if (kind.equals("Done") || kind.equals("No")) {
                return false;
            }
            spell.setActivatingPlayer(player);
            run.chooseTargets(player, spell);
            return ComputerUtil.playStack(spell, player, getGame());
        }

        @Override
        public List<OptionalCostValue> chooseOptionalCosts(SpellAbility chosen, List<OptionalCostValue> values) {
            return new ArrayList<>(); // the script's optional costs are added when it casts
        }

        @Override
        public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> sas) {
            if (sas.size() > 1) {
                // Beryl names the first trigger by its source; these share one, so Forge's order stands.
                run.take(player, Set.of("TriggerOrder"), "order " + sas);
            }
            return super.orderSimultaneousSa(sas);
        }

        @Override
        public void orderAndPlaySimultaneousSa(List<SpellAbility> sas) {
            for (SpellAbility sa : orderSimultaneousSa(sas)) {
                if (sa.isTrigger() && !sa.isCopied() && scriptedTargets(sa)) {
                    run.checkAsserts();
                    run.mode = Run.Mode.PLAY;
                    run.chooseTargets(player, sa);
                    ComputerUtil.playStack(sa, player, getGame());
                } else {
                    super.orderAndPlaySimultaneousSa(List.of(sa));
                }
            }
        }

        boolean scriptedTargets(SpellAbility sa) {
            boolean targets = false;
            for (SpellAbility part = sa; part != null; part = part.getSubAbility()) {
                targets |= part.usesTargeting();
            }
            return targets && run.isAnswer(run.peekPastAsserts(), player, Set.of("Target", "TargetCount"));
        }

        @Override
        public boolean addKeywordCost(SpellAbility sa, Cost cost, forge.game.keyword.KeywordInterface keyword, String prompt) {
            boolean wanted = run.keywordCost != null && keyword.getOriginal().startsWith(run.keywordCost);
            if (wanted) {
                run.keywordCost = null;
            }
            return wanted;
        }

        @Override
        public Card chooseSingleCardForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, DelayedReveal delayedReveal, String selectPrompt, boolean isOptional, Player decider) {
            List<Card> picked = pickMany(List.copyOf(fetchList), isOptional ? 0 : 1, 1, "move " + selectPrompt);
            if (picked == null) {
                return super.chooseSingleCardForZoneChange(destination, origin, sa, fetchList, delayedReveal, selectPrompt, isOptional, decider);
            }
            return picked.isEmpty() ? null : picked.get(0);
        }

        @Override
        public List<Card> chooseCardsForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, int min, int max, DelayedReveal delayedReveal, String selectPrompt, Player decider) {
            List<Card> picked = pickMany(List.copyOf(fetchList), min, max, "move " + selectPrompt);
            return picked != null ? picked : super.chooseCardsForZoneChange(destination, origin, sa, fetchList, min, max, delayedReveal, selectPrompt, decider);
        }

        @Override
        public Map<Card, forge.card.mana.ManaCostShard> chooseCardsForConvokeOrImprovise(SpellAbility sa, forge.card.mana.ManaCost manaCost, CardCollectionView untapped, boolean artifacts, boolean creatures, Integer maxReduction) {
            if (!run.isAnswer(run.peekPastAsserts(), player, Set.of("Waterbend"))) {
                return super.chooseCardsForConvokeOrImprovise(sa, manaCost, untapped, artifacts, creatures, maxReduction);
            }
            run.checkAsserts();
            Map<Card, forge.card.mana.ManaCostShard> tapped = new java.util.LinkedHashMap<>();
            List<Card> remaining = new ArrayList<>(untapped);
            for (JsonObject pick = run.takeFollowing(player, Set.of("Waterbend")); pick != null && pick.get("pick").getAsString().equals("Card"); pick = run.takeFollowing(player, Set.of("Waterbend"))) {
                Card card = (Card) run.resolvePick(pick, remaining);
                remaining.remove(card);
                tapped.put(card, forge.card.mana.ManaCostShard.GENERIC);
            }
            // Beryl taps one more while mana could not pay the rest without it: it offers the
            // only way left without asking.
            int mana = 0;
            for (Card card : player.getCardsIn(ZoneType.Battlefield)) {
                if (card.isUntapped() && !card.getManaAbilities().isEmpty() && !tapped.containsKey(card)) {
                    mana++;
                }
            }
            int left = manaCost.getGenericCost() - tapped.size();
            if (maxReduction != null) {
                left = Math.min(left, maxReduction - tapped.size());
            }
            for (int i = 0; i < left - mana && i < remaining.size(); i++) {
                tapped.put(remaining.get(i), forge.card.mana.ManaCostShard.GENERIC);
            }
            return tapped;
        }
    }
}
