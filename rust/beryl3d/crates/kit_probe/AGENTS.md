# Working in `kit_probe`

The parent [Beryl 3D guide](../../AGENTS.md) also applies here.

## Toolkit stories

A toolkit story is a UTF-8 `.story` file in the `stories` directory of its crate
or feature. Give it a name with lowercase letters, digits or hyphens that is
unique within the crate. The inspector finds the file as `story/<crate>/<name>`.
The header gives the default window in logical points and the desktop or phone
profile. The profile also selects which windows test the story.

### Element

```text
story element 360x240 desktop
region "Controls" #1 12,12 320x160 | fill=#272b32ff radius=8
  button "Save" #2 24,24 160x48 interactive primary | widget=button
```

Use a tree element line with two spaces per parent level. Start with a role, an
optional quoted name and a unique `#id`. Then give the place, the top left
corner and the size. Toolkit elements use the flat layer, for which the tree
leaves the place blank. An explicit `layer` also works. All boxes use window
coordinates, including children's boxes. A name or text value escapes quotes and
backslashes with a backslash.

An element can have `interactive`, `hovered`, `held`, `selected`, `disabled`,
`focused`, `primary` and `faded(<percentage>%)` states. The frame computes
coverage. Add styles after `|`, as space-separated properties. Use each property
at most once.

- `fill=#rrggbbaa` and `ink=#rrggbbaa` set the fill and ink color.
- `radius=8` rounds corners. `border=1,#rrggbbaa` sets a uniform border.
- `linear=90,#rrggbbaa,#rrggbbaa` sets an angle and two gradient stops.
- `shadow=0,2,6,#rrggbbaa` gives x and y offsets, blur and color.
- `text="words"` gives the words to draw instead of the element's name.
- `widget=<name>` uses a toolkit widget with that element's role. Use `button`,
  `small-button`, `phone-action`, `pass-on`, or `icon-button` for `button`. Use
  `checkbox` or `toggle` for `toggle`. Use `field` for `field`, `list` or `menu`
  for `list`, `slider` for `slider`, `tooltip` for `text`, and `start-panel`,
  `check`, or `check-square` for `region`. A slider accepts `level=0` through
  `level=100`.

### Container

```text
story container 360x240 desktop
container row 12,12 320x160 gap=6 max=40
item 40x56 group=first
item 44x56 group=first
```

The `row` or `fan` policy has a region, a gap in points and a maximum item
count. Each item gives its size in points and a group made of ASCII letters,
digits, hyphens or underscores. While the arrangement solver is pending, the
preview draws the region and reports the item count.

### Effect

```text
story effect 360x240 desktop
effect glow 114,60 132x120
```

Use one effect name from `kit_fx::effects::Effect` and a rectangle in window
points. The effect plays for 600 ms. A trail moves its rectangle so it can draw.
On a solid rectangle, dissolve fades the paint and cover uses the wash.

Use one LF after every line, including the last, and write numbers without extra
zeroes. Coordinates are finite. Widths and heights are positive and numbers are
bounded to 16,384 points in magnitude. Use up to 512 elements or items and 32
indentation levels. The parser names the line and the expected form when a file
differs from the grammar.

`just b3d list story/` lists stories. Use their names with `info`, `tree`,
`diff`, `png`, `film` or `check`. An element or container has one moment,
`start`, also named `end`. An effect also accepts times within its duration.
`--do` can move or hover the pointer and advance the clock. Use
`just b3d test story/` to test every story in its profile's windows. Add
`--phone-only` to keep just the phone stories.
