---
name: release
description: File Ignite tickets at your judgment from the discussion so far, then release them and any tickets this session already created. Use when the user invokes /release.
---

# Release

File tickets at your judgment from the discussion above, then release any of
them still pending, along with any pending ticket this session already created
(`ignite ticket update <id> --release`). A single ticket under no epic is filed
`open` and needs no release; an epic is filed pending, and releasing it releases
its tickets. The user's `/release` overrides the usual rule that agents never
pass `--release`, but only for tickets filed in this session.
