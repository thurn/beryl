---
name: release
description: File Ignite tickets at your judgment from the discussion so far, then release them and any tickets this session already created. Use when the user invokes /release.
---

# Release

File tickets at your judgment from the discussion above, then activate any of
them still parked, along with parked work this session already filed. The
user's `/release` authorizes activating tickets filed in this session.

- Set a parked epic's Allocation with `ignite ticket update <epic> --allocation <weight>`.
  Use the agreed weight, a positive weight already recorded on the ticket, or
  3 if neither exists. This also opens pending descendants. A descendant with
  its own zero stays parked until its allocation changes.
- For an individual ticket still parked after any epic activation, use its
  agreed or recorded positive weight. Otherwise clear its local allocation to
  use the normal ticket weight of 100. Clearing Allocation in the dashboard
  saves `{"allocation": null}`, which can also be sent in an `ignite ticket batch`
  update. Use the same blank value for a pending milestone that should share
  its parent's workers.
