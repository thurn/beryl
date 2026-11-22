"""Creates Beryl 3D source and story scaffolds, with recipes beside their writers."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shlex
import sys
import tempfile
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from .. import paths, system
from .command import Command
from .new_files import CRATES, Files, Invalid

DATA = Path("data/beryl3d")
CONVENTIONS = Path("data/ui-conventions.json")
PROTOCOL = Path("rust/crates/beryl_protocol/src")
PROFILE_SOURCE: Path = DATA / "profiles/wide-compact.json"
EFFECT_SOURCE: Path = DATA / "effects/glow.json"
REGISTRY: Path = CRATES / "app_client/src/client/features.rs"
NAME: re.Pattern[str] = re.compile(r"[a-z][a-z0-9]*(?:-[a-z0-9]+)*")
RUST_KEYWORDS: frozenset[str] = frozenset(
    "abstract as async await become box break const continue crate do dyn else enum extern "
    "false final fn for gen if impl in let loop macro match mod move mut override priv pub "
    "ref return self static struct super trait true try type typeof unsafe unsized use "
    "virtual where while yield".split()
)


def add_feature(files: Files, _: argparse.Namespace) -> None:
    name, module, feature = files.name, files.module, files.variant
    screens = CRATES / "app_screens/src/screens"
    folder = screens / module
    files.require_new_word(REGISTRY, f'"{name}"', f'"{feature}"')
    files.require_new_module(screens)
    files.require_new_module(REGISTRY.parent / "features")
    files.append(screens / "mod.rs", f"\npub mod {module};\n")
    files.add(
        folder / "mod.rs",
        f"""//! The {name} feature.

mod view;

use std::convert::Infallible;

use kit_scene::{{Context, Feature}};
use kit_ui::element::Element;

pub struct {feature};

impl Feature for {feature} {{
    const SOURCE: &'static str = file!();

    type State = ();
    type Message = Infallible;
    type Inputs<'a> = ();
    type ViewKey = ();
    type Command = Infallible;
    type Request = Infallible;

    fn update(_: &mut (), message: &Infallible, _: &mut Context<'_, Infallible, Infallible>) -> bool {{
        match *message {{}}
    }}

    fn view_key(_: &(), _: &()) {{}}

    fn view(_: &(), (): ()) -> Element {{
        view::view()
    }}
}}
""",
    )
    files.add(
        folder / "view.rs",
        f"""use kit_ui::element::{{Element, text}};

pub(super) fn view() -> Element {{
    text("{name}")
}}
""",
    )
    mount = f"app_screens::screens::{module}::{feature}"
    files.insert(
        REGISTRY,
        "    Registration::of::<SettingsSheet>",
        f'    Registration::routed::<{mount}>("{name}"),\n',
    )
    files.insert(REGISTRY, "mod hud;\n", f"mod {module};\n")
    files.add(
        REGISTRY.parent / "features" / f"{module}.rs",
        f"""//! Mounts the {name} feature for its story.

use super::{{Client, Element, Features, Mounted, ViewInputs}};
use {mount};

impl Mounted for {feature} {{
    fn mounted_view<'a>(
        features: &'a mut Features,
        id: &'static str,
        _: ViewInputs<'_>,
    ) -> Option<&'a Element> {{
        Some(Features::slot::<Self>(&mut features.slots, id).view(()))
    }}

    fn command(_: &mut Client, command: Self::Command) {{
        match command {{}}
    }}

    fn request(_: &mut Client, request: Self::Request) -> bool {{
        match request {{}}
    }}
}}
""",
    )
    files.story_file(
        "app_screens",
        name,
        f"story feature 1600x900 desktop\nfeature {name}\ncapture {name} 0 you 1 catch-up\n",
        folder / "stories",
    )
    files.test(
        "app_client",
        f"feature_{module}",
        f"""use app_client::registered_features;
use {mount};
use kit_scene::Feature;

#[test]
fn feature_is_registered_and_has_a_view() {{
    assert!(registered_features().any(|feature| feature.id == "{name}"));
    assert_eq!({feature}::view(&(), ()).name, "{name}");
}}
""",
    )


def element_story(name: str, widget: str = "") -> str:
    suffix = f" | widget={widget}" if widget else ""
    return f'story element 360x240 desktop\nregion "{name}" #1 12,12 120x48{suffix}\n'


def add_widget(files: Files, _: argparse.Namespace) -> None:
    name, module, variant = files.name, files.module, files.variant
    widgets = CRATES / "kit_ui/src/widgets"
    catalog = widgets / "catalog.rs"
    files.require_new_module(widgets)
    files.require_new_word(catalog, name, variant)
    files.add(
        widgets / f"{module}.rs",
        f"""//! The {name} widget.

use crate::element::{{Element, column}};

#[must_use]
pub fn {module}(name: &str) -> Element {{
    column().name(name)
}}
""",
    )
    files.append(widgets / "mod.rs", f"\nmod {module};\npub use {module}::{module};\n")
    files.insert(
        catalog, '    Button = "button"', f'    {variant} = "{name}" in {module};\n'
    )
    files.insert(
        catalog,
        "            Self::Field => Role::Field,",
        f"            Self::{variant} => Role::Region,\n",
    )
    files.insert(
        CRATES / "kit_probe/src/story/style.rs",
        "        Widget::Button => button(),",
        f"        Widget::{variant} => widgets::{module}(name),\n",
    )
    files.story_file("kit_ui", name, element_story(name, name))
    files.test(
        "kit_ui",
        f"widget_{module}",
        f"""use kit_ui::widgets::{{Widget, {module}}};

#[test]
fn widget_uses_its_catalog_role() {{
    assert_eq!({module}("{name}").role, Widget::{variant}.role());
}}
""",
    )


def container_story(name: str = "row") -> str:
    return (
        "story container 360x240 desktop\n"
        f"container {name} 12,12 320x160 gap=6 max=4\n"
        "item 40x56 group=first\nitem 44x56 group=second\n"
    )


def add_policy(files: Files, _: argparse.Namespace) -> None:
    name, variant = files.name, files.variant
    source = CRATES / "kit_space/src/arrange.rs"
    files.require_new_word(source, name, variant)
    files.insert(
        source,
        "    /// A single rectangle centred in the region.",
        f'    /// Arranges rectangles in rows.\n    {variant} = "{name}" [{{ columns: usize, gap: Vec2 }}],\n',
    )
    layout = source.parent / "arrange/layout.rs"
    files.insert(
        layout,
        "                Policy::Grid { .. } => ScrollAxis::Vertical,",
        f"                Policy::{variant} {{ .. }} => ScrollAxis::Vertical,\n",
    )
    files.insert(
        layout,
        "            Policy::Grid { columns, gap } => self.place_grid",
        f"            Policy::{variant} {{ columns, gap }} => self.place_grid(pack, columns, gap),\n",
    )
    test = CRATES / "kit_space/tests/all/arrange.rs"
    files.insert(
        test,
        "        Policy::Spot,\n    ];",
        f"""        Policy::{variant} {{
            columns: 1,
            gap: Vec2::ZERO,
        }},
""",
    )
    files.append(
        test,
        f"""
#[test]
fn {files.module}_places_two_rectangles() {{
    let camera = reference_camera();
    let policy = Policy::{variant} {{
        columns: 1,
        gap: Vec2::ZERO,
    }};
    let container = container(&camera, policy, Limits::default(), Vec2::splat(400.0));
    let items = rectangles(2, natural(&camera, Vec2::splat(60.0)));
    let solved = arrange(&container, &items, &camera, 0);
    assert_eq!(solved.overflow, Overflow::FullSize);
    assert!(solved.items.iter().all(|item| item.in_view));
}}
""",
    )
    story_source = CRATES / "kit_probe/src/story"
    files.replace(
        story_source / "mod.rs", "enum Policy {\n", f"enum Policy {{\n    {variant},\n"
    )
    files.insert(
        story_source / "mod.rs",
        '            Self::Row => "row",',
        f'            Self::{variant} => "{name}",\n',
    )
    files.insert(
        story_source / "parse.rs",
        '            "row" => Policy::Row,',
        f'            "{name}" => Policy::{variant},\n',
    )
    files.story_file("kit_space", name, container_story(name))


def table_story() -> str:
    return 'story table 1600x900 desktop\ncard "Island" #1 battlefield.yours.lands\n'


def add_profile(files: Files, _: argparse.Namespace) -> None:
    name, module = files.name, files.module
    definition = DATA / f"profiles/{name}.json"
    data = json.loads(files.read(PROFILE_SOURCE))
    data["name"] = name
    files.add(definition, json.dumps(data, indent=2) + "\n")
    profiles = CRATES / "game_table/src/profiles"
    files.require_new_module(profiles)
    files.append(profiles / "mod.rs", f"\npub mod {module};\n")
    files.add(
        profiles / f"{module}.rs",
        f"""//! The explicit {name} layout profile.

/// The profile's name and data path, for tools that opt into this layout.
pub const SOURCE: (&str, &str) = ("{name}", "profiles/{name}.json");
""",
    )
    pages = CRATES / "beryl_inspect/src/pages.rs"
    registry = re.search(
        r"static INSPECTION_PROFILES: \[InspectionProfile; (\d+)\] = \[",
        files.read(pages),
    )
    if registry is None:
        raise Invalid(f"{pages}: missing inspection profile registry")
    count = int(registry[1])
    files.replace(
        pages,
        registry[0],
        f"static INSPECTION_PROFILES: [InspectionProfile; {count + 1}] = [\n"
        f"    InspectionProfile {{\n        source: game_table::profiles::{module}::SOURCE,\n"
        "        data: OnceLock::new(),\n    },\n",
    )
    lines = table_story().splitlines(keepends=True)
    files.story_file(
        "game_table", name, f"{lines[0]}profile {name}\n" + "".join(lines[1:])
    )
    files.test(
        "beryl_inspect",
        f"profile_{module}",
        f"""use beryl_inspect::cli::main;

#[test]
fn profile_is_available_in_the_inspector() {{
    let args = ["info", "story/game_table/{name}"].map(str::to_owned);
    let ran = main(&args);
    assert_eq!(ran.code, 0, "{{}}", ran.stderr);
}}
""",
    )


def effect_story(name: str) -> str:
    return f"story effect 360x240 desktop\neffect {name} 114,60 132x120\n"


def add_effect(files: Files, _: argparse.Namespace) -> None:
    name, module = files.name, files.module
    data = json.loads(files.read(EFFECT_SOURCE))
    data["name"] = name
    data["preview"] = True
    files.add(DATA / f"effects/{name}.json", json.dumps(data, indent=2) + "\n")
    files.story_file("kit_fx", name, effect_story(name))
    files.test(
        "beryl_inspect",
        f"effect_{module}",
        f"""use beryl_inspect::cli::main;

#[test]
fn effect_story_loads_the_definition() {{
    let args = ["tree", "story/kit_fx/{name}", "--at", "200ms"].map(str::to_owned);
    let ran = main(&args);
    assert_eq!(ran.code, 0, "{{}}", ran.stderr);
}}
""",
    )


def add_verb(files: Files, _: argparse.Namespace) -> None:
    name, module, variant = files.name, files.module, files.variant
    verbs = CRATES / "game_play/src/verbs"
    files.require_new_module(verbs)
    names = PROTOCOL / "script/verb.rs"
    files.require_new_word(names, name, variant)
    # Keep the wire name equal to the file, including hyphens.
    files.insert(
        names,
        "    }\n}\n\nnames! {\n    /// What a mark",
        f'        /// A step with its own presentation.\n        {variant} = "{name}",\n',
    )
    script = PROTOCOL / "script"
    files.insert(
        script / "act.rs",
        "    Announce {\n",
        f"""    /// Presents a subject without changing its record.
    #[serde(rename = "{name}")]
    #[ts(rename = "{name}")]
    {variant} {{
        subject: Subject,
    }},
""",
    )
    files.insert(
        script / "act.rs",
        "            Act::Announce { .. } => Verb::Announce,",
        f"            Act::{variant} {{ .. }} => Verb::{variant},\n",
    )
    for path, indent in ((script / "table.rs", 12), (verbs.parent / "sound.rs", 8)):
        files.insert(
            path,
            " " * indent + "| Act::Dwell { .. }",
            f"{' ' * indent}| Act::{variant} {{ .. }}\n",
        )
    files.insert(
        script / "text.rs",
        "            Act::Dwell { subject } =>",
        f'            Act::{variant} {{ subject }} => write!(f, "{{verb}} {{subject}}"),\n',
    )
    files.insert(
        script / "timing.rs",
        "        Act::Lift { .. } | Act::Flip { .. }",
        f"        Act::{variant} {{ .. }} => None,\n",
    )
    index = verbs / "mod.rs"
    files.insert(index, "mod dwell;", f"mod {module};\n")
    for member in ("play", "BUILT"):
        files.insert(
            index,
            f"        Verb::Dwell => dwell::{member},",
            f"        Verb::{variant} => {module}::{member},\n",
        )
    files.add(
        verbs / f"{module}.rs",
        f"""//! The {name} verb starts as an instant change.

use super::{{StepIn, StepOut}};

pub const BUILT: bool = false;

pub fn play(_: &StepIn<'_>, out: &mut StepOut<'_>) {{
    out.instant();
}}
""",
    )
    # The shared timing reader uses the conventions entry as the default timing.
    entry = {
        "name": name,
        "shows": "The subject has no motion yet.",
        "values": {"duration": 0, "delay": None, "stagger": None, "curve": "none"},
    }
    formatted = "\n".join(
        f"    {line}" for line in json.dumps(entry, indent=2).splitlines()
    )
    files.insert(CONVENTIONS, '\n  ],\n  "effects": [', ",\n" + formatted)
    files.story_file(
        "game_play",
        name,
        table_story()
        + 'script {"beats":[[{"act":{"verb":"'
        + name
        + '","subject":{"kind":"object","slot":1}}}]]}\n',
    )
    files.test(
        "beryl_inspect",
        f"verb_{module}",
        f"""use beryl_inspect::cli::main;
use beryl_protocol::script::Verb;
use game_play::verbs;

#[test]
fn verb_has_an_instant_presentation() {{
    assert!(!verbs::built(Verb::{variant}));
    let args = ["info", "story/game_play/{name}"].map(str::to_owned);
    let ran = main(&args);
    assert_eq!(ran.code, 0, "{{}}", ran.stderr);
    assert!(ran.stdout.contains("{name}"));
}}
""",
    )


def add_prompt_shape(files: Files, _: argparse.Namespace) -> None:
    name, module, variant = files.name, files.module, files.variant
    wire = name.split("-")[0] + "".join(
        part.capitalize() for part in name.split("-")[1:]
    )
    game = CRATES / "game_hud/src/prompts"
    files.require_new_module(game)
    view = PROTOCOL / "prompt/view.rs"
    files.require_new_word(view, variant, f'"{wire}"')
    files.insert(
        view,
        "    /// The session holds no state for this prompt",
        f'    /// The {name} prompt body.\n    #[serde(rename = "{wire}")]\n'
        f'    #[ts(rename = "{wire}")]\n    {variant},\n',
    )
    files.insert(
        view,
        "            ShapeBody::Candidates => return None,",
        f"            ShapeBody::{variant} => return None,\n",
    )
    files.insert(
        PROTOCOL / "prompt/text.rs",
        "            ShapeBody::Candidates =>",
        f"            ShapeBody::{variant} => Ok(()),\n",
    )
    files.insert(
        CRATES / "game_probe/src/signature.rs",
        "            ShapeBody::Candidates =>",
        f"            ShapeBody::{variant} => json!([]),\n",
    )
    files.insert(
        REGISTRY.parent / "features/prompt.rs",
        "        ShapeBody::Candidates => Some(Placement::Candidates),",
        f'        ShapeBody::{variant} => Some(Placement::Shape("{wire}")),\n',
    )
    files.insert(game / "mod.rs", "mod number;\n", f"mod {module};\n")
    files.insert(
        game / "mod.rs", "pub use number::Number;\n", f"pub use {module}::{variant};\n"
    )
    files.insert(
        game / "mod.rs",
        "        ShapeBody::Number { .. } => Number::own",
        f"        ShapeBody::{variant} => {variant}::own(shown, was, steps),\n",
    )
    files.insert(
        REGISTRY,
        "    Registration::prompt::<prompts::Number>",
        f'    Registration::prompt::<prompts::{variant}>("prompt-{name}", Placement::Shape("{wire}")),\n',
    )
    files.add(
        game / module / "mod.rs",
        f"""//! The {name} prompt shape.

mod view;

prompt_feature!({variant}, view::view, super::no_steps);
""",
    )
    files.add(
        game / module / "view.rs",
        f"""use beryl_protocol::prompt::PromptView;
use kit_ui::element::Element;

use crate::hud::HudIn;

pub(super) fn view(_: &PromptView, _: &HudIn<'_>, bar: Element) -> Element {{
    bar.name("prompt-{name}")
}}
""",
    )
    decision = {
        "id": {"session": 1, "seq": 1},
        "kind": "chooseObject",
        "candidates": [],
        "halt": False,
        "cancellable": False,
        "undo": 0,
        "undoDepth": 0,
        "refused": [],
        "prompt": {
            "revision": 0,
            "purpose": "priority",
            "part": "pick",
            "title": name,
            "words": {
                "confirm": "",
                "counted": "",
                "decline": "",
                "yes": "",
                "no": "",
                "auto": "",
                "cancel": "",
                "piles": ["", ""],
            },
            "about": [],
            "marks": [],
            "buttons": [],
            "body": {"shape": wire},
        },
    }
    files.story_file(
        "game_hud",
        f"prompt-{name}",
        table_story() + f"prompt {json.dumps(decision, separators=(',', ':'))}\n",
        game / module / "stories",
    )
    files.test(
        "beryl_inspect",
        f"prompt_{module}",
        f"""use beryl_inspect::cli::main;

#[test]
fn prompt_shape_story_opens_from_the_wire_body() {{
    let args = ["tree", "story/game_hud/prompt-{name}", "--at", "end"].map(str::to_owned);
    let ran = main(&args);
    assert_eq!(ran.code, 0, "{{}}", ran.stderr);
    assert!(ran.stdout.lines().skip(2).any(|line| line.contains("prompt-{name}")));
}}
""",
    )


STORY_KINDS = ("element", "container", "effect", "table", "feature")


def capture_story(args: argparse.Namespace, name: str) -> str:
    from .cli import launch_program, quick_build

    if quick_build(["build", "-q", "-p", "beryl_inspect", "--bin", "b3d"]):
        raise Invalid("could not build the story exporter")
    with tempfile.TemporaryDirectory() as directory:
        output = Path(directory) / f"{name}.story"
        command = ["story", "from", args.source, "--at", args.at, "--out", str(output)]
        if args.feature:
            command += ["--feature", args.feature]
        done = launch_program("b3d", command, want=1, capture=True)
        if done.returncode:
            raise Invalid(
                done.stderr.strip()
                or f"could not capture {args.source!r} at {args.at!r}"
            )
        return output.read_text()


def add_story(files: Files, args: argparse.Namespace) -> None:
    kind = args.kind
    if not kind:
        raise Invalid("story needs --kind " + "|".join(STORY_KINDS))
    if bool(args.source) != bool(args.at):
        raise Invalid("--from and --at must be supplied together")
    if args.feature and kind != "feature":
        raise Invalid("--feature requires --kind feature")
    if args.feature and not NAME.fullmatch(args.feature):
        raise Invalid("--feature needs a registered feature id")
    if args.source and kind not in {"table", "feature"}:
        raise Invalid("recording captures use --kind table or --kind feature")
    if args.source:
        if kind == "feature" and not args.feature:
            raise Invalid("capturing a feature story needs --feature <id>")
        text = capture_story(args, files.name)
    elif kind == "feature":
        feature = args.feature or "settings"
        text = f"story feature 1600x900 desktop\nfeature {feature}\n"
        if feature != "settings":
            text += f"capture {files.name} 0 you 1 catch-up\n"
    else:
        text = {
            "element": lambda: element_story(files.name),
            "container": container_story,
            "effect": lambda: effect_story("glow"),
            "table": table_story,
        }[kind]()
    crate = {
        "element": "kit_ui",
        "container": "kit_space",
        "effect": "kit_fx",
        "table": "game_table",
        "feature": "game_probe",
    }[kind]
    files.story_file(crate, files.name, text)


@dataclass(frozen=True)
class Recipe:
    kind: str
    summary: str
    instructions: str
    write: Callable[[Files, argparse.Namespace], None]


RECIPES = (
    Recipe(
        "feature",
        "A screen, overlay or HUD feature with its own state and view.",
        "Fill in State, Message and view.rs. Choose a live placement in the client registry.\n"
        "See it with just b3d tree story/app_screens/<name> or just b3d png story/app_screens/<name>.\n"
        "Run just b3d cargo-test -p app_client and ignite test b3d-source.",
        add_feature,
    ),
    Recipe(
        "widget",
        "A reusable element with a catalogue role and a preview.",
        "Fill in the widget function and update its story adapter if the inputs change.\n"
        "See it with just b3d tree story/kit_ui/<name> or just b3d png story/kit_ui/<name>.\n"
        "Run just b3d cargo-test -p kit_ui and just b3d test story/kit_ui/<name>.",
        add_widget,
    ),
    Recipe(
        "policy",
        "An arrangement solver policy with a container story.",
        "Replace the starter grid placement with your policy and extend the arrangement tests.\n"
        "See the story region with just b3d tree story/kit_space/<name>.\n"
        "Run just b3d cargo-test -p kit_space and just b3d test story/kit_space/<name>.",
        add_policy,
    ),
    Recipe(
        "profile",
        "An explicit preview layout cloned from the wide compact profile.",
        "Edit the slots, table geometry and test windows in the new JSON file.\n"
        "See it with just b3d tree story/game_table/<name> or just b3d png story/game_table/<name>.\n"
        "Run ignite test b3d-source and just b3d test story/game_table/<name>.",
        add_profile,
    ),
    Recipe(
        "effect",
        "A data-defined effect and a timed preview.",
        "Edit the JSON layers. Remove preview and register shared timing to ship the effect.\n"
        "See it with just b3d png story/kit_fx/<name> --at 200ms.\n"
        "Run ignite test b3d-source and just b3d test story/kit_fx/<name>.",
        add_effect,
    ),
    Recipe(
        "verb",
        "A script verb with an instant presentation to fill in.",
        "Implement play and its timing. Add session and web support before emitting it live.\n"
        "See it with just b3d tree story/game_play/<name> or just b3d png story/game_play/<name>.\n"
        "Run just b3d cargo-test -p beryl_inspect and ignite test b3d-timeline.",
        add_verb,
    ),
    Recipe(
        "story",
        "An editable scene that can run without the game engine.",
        "Fill in the story's element, container, effect, table or feature lines.\n"
        "To capture a recording, use --kind table --from <scene> --at <moment>.\n"
        "For a feature capture, use --kind feature and give --feature <id> as well.\n"
        "See it with just b3d tree story/<crate>/<name> or just b3d png story/<crate>/<name>.\n"
        "Run just b3d test story/<crate>/<name>.",
        add_story,
    ),
    Recipe(
        "prompt-shape",
        "A prompt body with a mounted view and a first story.",
        "Fill in the body and view. Connect a purpose and its shape in the session to use it live.\n"
        "See it with just b3d tree story/game_hud/prompt-<name> --at end.\n"
        "Run just b3d cargo-test -p beryl_inspect and just b3d test story/game_hud/prompt-<name>.",
        add_prompt_shape,
    ),
)


def recipe_formatter(prog: str) -> argparse.HelpFormatter:
    return argparse.RawDescriptionHelpFormatter(prog, width=88)


def parser() -> tuple[argparse.ArgumentParser, dict[str, argparse.ArgumentParser]]:
    result = argparse.ArgumentParser(
        prog="just b3d new", description="Scaffold a client part or a story."
    )
    subparsers = result.add_subparsers(dest="scaffold_kind")
    children: dict[str, argparse.ArgumentParser] = {}
    for recipe in RECIPES:
        child = subparsers.add_parser(
            recipe.kind,
            help=recipe.summary,
            description=recipe.summary,
            epilog=recipe.instructions,
            formatter_class=recipe_formatter,
        )
        children[recipe.kind] = child
        child.add_argument("name", help="lowercase letters, numbers and hyphens")
        if recipe.kind == "story":
            child.add_argument("--kind", choices=STORY_KINDS, required=True)
            child.add_argument("--from", dest="source", metavar="SCENE")
            child.add_argument("--at", metavar="MOMENT")
            child.add_argument("--feature", metavar="ID")
    return result, children


def help_by_kind() -> dict[str, str]:
    _, children = parser()
    return {
        kind: child.format_help() if child.epilog else ""
        for kind, child in children.items()
    }


def run(argv: list[str], root: Path) -> int:
    arguments, _ = parser()
    if not argv:
        print("just b3d new")
        for recipe in RECIPES:
            print(f"  {recipe.kind:<13} {recipe.summary}")
        return 0
    args = arguments.parse_args(argv)
    try:
        if not NAME.fullmatch(args.name) or args.name in RUST_KEYWORDS:
            raise Invalid(
                "use a lowercase name starting with a letter, with single hyphens between parts"
            )
        recipe = next(recipe for recipe in RECIPES if recipe.kind == args.scaffold_kind)
        files = Files(root, args.name)
        digest = hashlib.sha256(str(root.resolve()).encode()).hexdigest()[:16]
        with system.file_lock(paths.cache_root() / f"b3d-new-{digest}.lock"):
            recipe.write(files, args)
            files.write()
    except (OSError, Invalid, json.JSONDecodeError) as error:
        print(f"just b3d new: {error}", file=sys.stderr)
        return 2
    print(shlex.join(["just", "b3d", "new", *argv]))
    for path in files.changed:
        print(path)
    print(f"just b3d tree {files.story}")
    return 0


def run_command(args: argparse.Namespace) -> int:
    return run(args.arguments, paths.REPO)


COMMANDS = (
    Command(
        "new", "scaffold a client part or a story", (), run_command, passthrough=True
    ),
)
