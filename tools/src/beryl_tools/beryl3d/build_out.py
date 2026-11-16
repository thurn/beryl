"""Whether Beryl 3D is still in its build-out, when a new verb or screen lands every hour or so
(docs/beryl-3d.md, 'During the build-out').

While it is, the sweep's exact-count gates fail only on what a feature commit cannot explain:

- `just b3d gate --counts` lets an instruction count pass its limit by
  [`BUILD_OUT_TOLERANCE_PERCENT`] instead of [`TIGHT_TOLERANCE_PERCENT`], and a count below its
  limit does not fail (counts.py);
- `just b3d gate --gpu` reports a board's fragments over its limit as a warning, and fails only
  a board over the Baseline target itself (gpu.py);
- the scaling check, which fails when cost grows faster than the board, holds at every velocity
  (counts.py, `scaling_failures`).

Set [`BUILD_OUT`] to `False` when feature work slows down, and at the latest when the owner
switches the default client to Beryl 3D. Then lower every limit with `--counts --update` and
`--gpu --update` in the same commit, so that the tight gates start from what is measured.
"""

BUILD_OUT = True
# How far an instruction count may pass its limit during the build-out. One verb raised a
# board's total by 5 to 66% in November 2026, all of it new work.
BUILD_OUT_TOLERANCE_PERCENT = 20.0
# Afterwards: enough for inlining and layout shifts from unrelated changes, as the engine's
# count gate allows.
TIGHT_TOLERANCE_PERCENT = 1.0


def tolerance_percent() -> float:
    """How far an instruction count may pass its limit now."""
    return BUILD_OUT_TOLERANCE_PERCENT if BUILD_OUT else TIGHT_TOLERANCE_PERCENT
