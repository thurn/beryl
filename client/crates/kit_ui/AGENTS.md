# Working in `kit_ui`

This toolkit crate builds flat interface elements, widgets, styles and layout.
It also handles focus, text entry, scrolling and transitions. Supply plain
labels and typed actions. Game and app features choose what they mean.

Start with `element::Element`, `element::Style` and the factories in `widgets`.
`layout::Layer` lays out and writes resolved elements and paint. Preserve each
widget's role, focus and action through layout and clipping.

`tests/all/` covers elements, focus, paints, backdrops, transitions and
truncation. Layout and widget internals have tests near their source. Changing a
shared widget can affect several screens. Check compact wrapping and touch
targets as well as the desktop result.
