// How a card's flight bows off its straight line (tracks.ts `glide` plays it): a spell
// thrown onto the stack, a permanent set down from it, a card drawn or taken into a hand.
import type { Pose } from "./pose.ts";
import type { Act } from "./table.ts";
import { draws } from "./tracks.ts";

/** How a flight leaves the straight line between its ends: by `share` of that line where it
 * is farthest off it, `peak` of the way along (half way, if not said), leaning `lean`
 * degrees into the bow there and upright again as it lands. It bows `toward` the edge
 * of the table its card's controller sits at (1 down the stage, -1 up it): the outside of
 * the table, where it crosses the fewest cards. One that swings `back` also leaves away from
 * where it is going, across the table, by that share of how far across the line runs where
 * it is farthest off it: with a bow toward the middle of the table, the card comes out of a
 * hand into the open table and hooks round to its place. */
export interface Bow {
  share: number;
  peak?: number;
  lean?: number;
  back?: number;
  toward: number;
}

/** The bows of the flights that have one (the motion reviews of fly and emerge,
 * by-w8gv.21.47: "a card cast from hand is a thrown object, and here it slides along a
 * ruler line"): a spell thrown onto the stack, a permanent set down from it, which swings out
 * and settles into its row. An ability lifting out of its source goes all but straight: Arena's
 * copy, filmed whole from both seats, sags at most 0.025 of its way toward the middle of the
 * table (`IEXcjBn3BXk 20:39.43`, `5UHzpVvUkfI 19:48.57`), where a bow of 0.12 toward its
 * controller's edge put it 2 to 3% of the table's width off Arena's. An opponent's spell bows half as far, and later: their hand lies over the
 * table's far edge, and a full bow carried the card along that edge, half out of sight,
 * before it came down (the third review of fly, there). It leans the other way from a
 * thrown spell, its top away from the side it swings out to, so that edge-on it is a sliver
 * rising toward the stack, 7 to 10 degrees off level, as Arena's is (`UrGalw_f35w
 * 16:47.47`, `17:16.33`). Arena's own spell bows 0.29 of its
 * line, and its ability's copy and its resolving permanent travel a curve
 * (data/arena-motion.json, fly and emerge). A card drawn that no one reads, the opponent's,
 * leaves the line toward the middle of the table, away from its player's edge. So does the
 * land an opponent plays, and farther: Arena's drops out of their hand down into the table
 * on the side away from their lands, tilted, and swings back along the table to its place
 * (`UrGalw_f35w 16:46.33`, `pE4XSNi-nZc 20:13.60`: farthest out about 0.4 of its time in,
 * between 0.12 and 0.39 of the table's width beyond where it left the hand and 0.18 to 0.27
 * of its height below it). */
export const BOWS = {
  cast: { share: 0.15 },
  theirs: { share: 0.08, peak: 0.6, lean: -8 },
  played: { share: 0.3, back: 1, peak: 0.6, lean: 20 },
  lifts: { share: 0.02 },
  drawn: { share: 0.2, peak: 0.42 },
  taken: { share: 0.58, back: 0.45, peak: 0.5 },
};

/** How the flight of the step `act` bows, if it does, its card's controller sitting at the
 * edge `toward`. */
export function bowOf(act: Act, toward: number): Bow | undefined {
  // An ability sags toward the middle of the table, away from its controller's edge.
  if (act.verb === "emerge") return { ...BOWS.lifts, toward: -toward };
  if (act.verb !== "fly") return undefined;
  if (act.object.zone === "stack") {
    return { ...(toward < 0 ? BOWS.theirs : BOWS.cast), toward };
  }
  // A card no one reads that an effect takes from a library into a hand (the two cards of
  // the opponent's Stock Up) drops into the table first: Arena's falls almost straight down
  // from their library, is deepest half way, about 0.38 of the table's height below its
  // straight line and still over the library, then crosses and rises into the hand
  // (`3sEoP_s4DQE 46:04.03` to `46:04.70`). Beryl's library lies farther from the hand than
  // Arena's, so the same depth is a smaller share of its longer line.
  if (draws(act) && act.why !== "draw") {
    return { ...BOWS.taken, toward: -toward };
  }
  if (draws(act)) return { ...BOWS.drawn, toward: -toward };
  if (act.object.zone !== "battlefield") return undefined;
  // The player's own land glides straight from where it is let go (Arena's, `UrGalw_f35w
  // 16:35.42`).
  if (act.from === "hand") {
    return toward < 0 ? { ...BOWS.played, toward: -toward } : undefined;
  }
  return undefined;
}

/** `pose`, which is `through` of the straight way from `from` to `to`, as far off that way
 * as `bow` has it there: nothing at either end. */
export function bowed(
  pose: Pose,
  from: Pose,
  to: Pose,
  through: number,
  bow: Bow,
): Pose {
  const [across, down] = [to.x - from.x, to.y - from.y];
  // Of the two sides of the line, the one toward that edge of the table.
  const side = across * bow.toward < 0 ? -1 : 1;
  const sharpness = Math.log(0.5) / Math.log(bow.peak ?? 0.5);
  const arch = Math.sin(Math.PI * Math.max(0, through) ** sharpness);
  const off = side * bow.share * arch;
  const back = (bow.back ?? 0) * arch;
  return {
    ...pose,
    x: pose.x - down * off - across * back,
    y: pose.y + across * off,
    // It leans into the bow, its top toward the side it swings out to.
    rotation: pose.rotation - side * (bow.lean ?? 0) * arch,
  };
}
