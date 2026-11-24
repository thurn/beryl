# Working in `kit_scene`

This toolkit crate owns node relations, solve and hit order, pointer and scroll
routing, and the feature contract. Supply opaque keys and typed messages. Game
state and session commands belong to the callers.

Start with `Tree`, `Node`, `Feature` and `FeatureSlot`. A feature's state holds
local details such as a scroll offset. Its view derives session details from
inputs. Equal view keys for the same state must produce equal elements and
actions. A feature returns effects to its owner through `Context`.

`tests/all/` covers feature caching, tree relations, gestures and scrolling. A
change to solve or hit order can make a visible node unreachable. Check the
resolved frame and the hit at the same point.
