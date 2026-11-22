# Working in `kit_text`

This toolkit crate shapes text, breaks lines and emits glyph requests. It
accepts strings and spans without knowing which game or widget uses them. Hosts
rasterize glyphs and upload them to the atlas.

`text::Text::measure` and `Text::block` share the shaping path. A `Block`
contains the lines and positioned runs used by layout and drawing. Keep
measurement and drawing in agreement at the same width and scale.

`tests/all/` covers shaping, serif metrics and truncation. The symbol tests live
in `text/symbols.rs`. A metrics change can alter prompt wraps, truncation and
the recording corpus signature, which includes text line counts.
