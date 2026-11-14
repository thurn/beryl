// The kept checks: properties of the screens (the start screen, the
// settings, the log, the result, the playground, a bug report's freeze, keys, tips, stops) and
// measures of the layout, which no generic audit can express,
// `by ui-test` (tools/) serves the UI and runs this. Usage:
//   node scripts/kept.mjs <base-url> <fixtures-dir> [title-filter...]
// Filters after the fixtures folder run only the checks whose titles contain one of them, in any
// case. Exits non-zero if a check fails or a page logs an error. The harness is kept-harness.mjs;
// the checks are in the parts imported below, by what they hold.
import { finish } from "./kept-harness.mjs";

await import("./kept-screens.mjs");
await import("./kept-mobile.mjs");
await import("./kept-result.mjs");
await import("./kept-bug-reports.mjs");
await import("./kept-table.mjs");
await import("./kept-crowded.mjs");
await import("./kept-piles.mjs");
await import("./kept-pile-look.mjs");
await import("./kept-pointer.mjs");
await import("./kept-play.mjs");
await finish();
