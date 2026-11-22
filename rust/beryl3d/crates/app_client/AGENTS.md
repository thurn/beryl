# Working in `app_client`

This crate assembles the native client, registers features, routes input and
exchanges commands with the session and host. The host owns platform services
and the renderer consumes the client's frame.

Start with `Client::handle`, `Client::prepare` and `Client::frame`.
`registered_features` exposes feature ownership, `command` holds session
commands and `driver` replays recordings and stories. These entry points read no
clock, thread or file. Pass time and assets through inputs. Painting a prepared
frame allocates nothing.

`tests/all/` covers input, screens, prompts, table placement and replay through
the assembled client. `tests/frame_allocation.rs` and `tests/play_allocation.rs`
use separate allocators. A routing or preparation change can alter focus, action
delivery and animation wakeups across screens.
