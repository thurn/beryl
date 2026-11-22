# Working in `kit_probe`

This toolkit crate prints trees, parses locators and driver actions, and checks
resolved frames. `agent` reads the frame, `driver` defines replay controls and
`story` parses and renders standalone scenes. Give them plain elements and
opaque IDs. Magic recordings and checks that read protocol scripts belong in
`game_probe`.

`tests/all/` covers tree output, driver syntax and stories. A tree line is a
locator and an element-story input. Keep those forms usable after a change to
printing or parsing.

## Toolkit story grammar

Put a UTF-8 `.story` in the crate or feature's `stories/` directory. Its name
uses lowercase letters, digits and hyphens and is unique in the crate. Open it
as `story/<crate>/<name>`. The header selects the default window in logical
points and the desktop or phone test windows.

```text
story element 360x240 desktop
region "Controls" #1 12,12 320x160 | fill=#272b32ff radius=8
  button "Save" #2 24,24 160x48 interactive primary | widget=button
```

Use a tree element line with a role, optional quoted name, unique `#id`,
top-left corner and size. The flat-layer place is blank or `layer`. Indent two
spaces per parent level. All boxes use window coordinates. Escape quotes and
backslashes inside a name or text value with a backslash. States include
`interactive`, `hovered`, `held`, `selected`, `disabled`, `focused`, `primary`
and `faded(<percentage>%)`. Coverage is computed.

After `|`, use each style property at most once. The properties include
`fill=#rrggbbaa`, `ink=#rrggbbaa`, `radius=8`, `border=1,#rrggbbaa`,
`linear=90,#rrggbbaa,#rrggbbaa`, `shadow=0,2,6,#rrggbbaa` and `text="words"`.
With `widget=<name>`, use the widget's role, such as `button` for `small-button`
or `toggle` for `checkbox`. Sliders take `level=0` through `level=100`.

```text
story container 360x240 desktop
container row 12,12 320x160 gap=6 max=40
item 40x56 group=first
item 44x56 group=first
```

Choose `row` or `fan`, give a region, gap and item limit, and specify each
item's size. Group names use ASCII letters, digits, hyphens or underscores.

```text
story effect 360x240 desktop
effect glow 114,60 132x120
```

Use a loaded effect name and a rectangle in window points. The preview lasts 600
ms. Element and container stories have one moment, `start` or `end`. Use one LF
per line including the last, no extra zeroes, positive sizes and finite numbers
within 16,384 points. Use at most 512 elements or items and 32 indentation
levels. See [the parser](src/story/parse.rs) for widget names.
