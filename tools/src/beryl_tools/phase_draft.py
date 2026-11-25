"""Translate published typed JSON from phase.rs (MIT/Apache-2.0) into Beryl builders.

Each reader consumes the fields it understands. Unsupported or extra fields
leave that ability for the card writer, including nested fields and sub-abilities.
"""

from __future__ import annotations

import re

from dataclasses import dataclass
from typing import cast

from . import card_store

Json = card_store.Json


class Unsupported(ValueError):
    """The Phase node contains something this translator cannot express."""

    def __init__(self, message: str, node_kind: str = "") -> None:
        super().__init__(message)
        self.node_kind = node_kind


def node_kind(category: str, tag: object) -> str:
    """A Phase enum tag, ignoring fields carried by a variant."""
    if isinstance(tag, dict) and len(tag) == 1:
        tag = next(iter(cast(Json, tag)))
    return (
        f"{category}:{tag}"
        if isinstance(tag, str)
        and tag not in {"Unknown", "GenericEffect", "Unimplemented"}
        else ""
    )


class Node:
    def __init__(self, value: object, path: str) -> None:
        if not isinstance(value, dict):
            raise Unsupported(f"{path}: expected a Phase object")
        self.fields: dict[str, object] = dict(cast(Json, value))
        self.path = path

    def take(self, key: str, default: object = None) -> object:
        return self.fields.pop(key, default)

    def expect(self, key: str, expected: object, *, required: bool = False) -> None:
        actual = self.take(key, None if required else expected)
        if actual != expected:
            group = (
                node_kind(self.path.replace(" ", "_"), actual) if key == "type" else ""
            )
            raise Unsupported(f"{self.path}.{key}: unsupported value {actual!r}", group)

    def finish(self) -> None:
        if self.fields:
            raise Unsupported(
                f"{self.path}.{next(iter(self.fields))}: unsupported field"
            )


def number(value: object, least: int = 0, most: int = 255) -> int:
    if type(value) is not int or not least <= value <= most:
        raise Unsupported(f"unsupported number {value!r}")
    return value


def fixed(value: object, least: int = 0, most: int = 255) -> int:
    node = Node(value, "amount")
    node.expect("type", "Fixed")
    count = number(node.take("value"), least, most)
    node.finish()
    return count


def items(value: object) -> list[object]:
    if not isinstance(value, list):
        raise Unsupported(f"expected a list, got {value!r}")
    return cast(list[object], value)


COLORS: dict[str, str] = dict(
    zip(("White", "Blue", "Black", "Red", "Green", "Colorless"), "WUBRGC")
)
BASIC_LAND_COLORS = {
    "Plains": "White",
    "Island": "Blue",
    "Swamp": "Black",
    "Mountain": "Red",
    "Forest": "Green",
}
TYPES: dict[str, str] = {
    name: f"Types::{name.upper()}"
    for name in (
        "Artifact",
        "Battle",
        "Creature",
        "Enchantment",
        "Land",
        "Planeswalker",
    )
}


@dataclass(frozen=True)
class Target:
    spec: str = ""
    objects: str = ""
    players: str = ""
    kind: str = ""


def target(value: object, up_to: int = 0) -> Target:
    if value is None and not up_to:
        return Target(players="you()")
    node = Node(value, "target")
    kind = node.take("type")
    if up_to and kind != "Typed":
        raise Unsupported("ability.multi_target: expected a typed permanent target")
    result = Target()
    if kind == "Controller":
        result = Target(players="you()")
    elif kind == "Player":
        result = Target(
            "TargetSpec::player()", players="target_player()", kind="Player"
        )
    elif kind == "Opponent":
        result = Target(
            "TargetSpec::opponent()", players="target_player()", kind="Player"
        )
    elif kind == "Any":
        result = Target("TargetSpec::any_target()", kind="Any")
    elif kind == "SelfRef":
        result = Target(objects="this()")
    elif kind == "Typed":
        result = typed_target(node, up_to)
    else:
        raise Unsupported(
            f"target.type: unsupported value {kind!r}", node_kind("target", kind)
        )
    node.finish()
    return result


def typed_target(node: Node, up_to: int) -> Target:
    types = items(node.take("type_filters"))
    properties = items(node.take("properties", []))
    controller = node.take("controller")
    if not up_to and not types and not properties and controller == "Opponent":
        return Target(
            "TargetSpec::opponent()", players="target_player()", kind="Player"
        )
    if not up_to and types == ["Creature"] and not properties and controller is None:
        return Target("TargetSpec::creature()", "target()", kind="Permanent")
    filters: list[str] = []
    kind = "Spell" if {"type": "InZone", "zone": "Stack"} in properties else "Permanent"
    for card_type in types:
        if isinstance(card_type, str) and card_type in TYPES:
            filters.append(f"Filter::Types({TYPES[card_type]})")
        elif card_type == {"Non": "Land"}:
            filters.append("Filter::Not(&Filter::Types(Types::LAND))")
        elif (kind, card_type) not in (("Spell", "Card"), ("Permanent", "Permanent")):
            raise Unsupported(f"target.type_filters: unsupported type {card_type!r}")
    if controller == "You":
        filters.append("Filter::Whose(&Players::You)")
    elif controller == "Opponent":
        filters.append("Filter::Whose(&Players::EachOpponent)")
    elif controller is not None:
        raise Unsupported(f"target.controller: unsupported value {controller!r}")
    for prop in properties:
        if prop == {"type": "Tapped"}:
            filters.append("Filter::Tapped")
        elif prop == {"type": "Token"}:
            filters.append("Filter::Token")
        elif prop != {"type": "InZone", "zone": "Stack"}:
            property_node = Node(prop, "target property")
            property_node.expect("type", "NotColor")
            color = property_node.take("color")
            if (
                not isinstance(color, str)
                or color not in COLORS
                or color == "Colorless"
            ):
                raise Unsupported(f"target property.color: unsupported value {color!r}")
            property_node.finish()
            filters.append(f"Filter::Not(&Filter::Colors(Colors::{color.upper()}))")
    filter_code = ", ".join(filters)
    if len(filters) > 1:
        filter_code = f"Filter::All(&[{filter_code}])"
    if kind == "Permanent" and not types:
        raise Unsupported("target.type_filters: no permanent type")
    filter_code = filter_code or "Filter::Any"
    if up_to:
        if kind != "Permanent":
            raise Unsupported("ability.multi_target: expected a permanent target")
        spec = f"TargetSpec::up_to({up_to}, TargetKind::Permanent, {filter_code})"
    elif kind == "Spell" and not filters:
        spec = "TargetSpec::spell()"
    else:
        spec = f"TargetSpec::one(TargetKind::{kind}, {filter_code})"
    return Target(spec, "target()", kind=kind)


def mana_cost(value: object) -> str:
    node = Node(value, "mana cost")
    node.expect("type", "Cost")
    generic = number(node.take("generic", 0))
    shards = items(node.take("shards", []))
    node.finish()
    symbols = f"{{{generic}}}" if generic else ""
    for shard in shards:
        if not isinstance(shard, str) or shard not in COLORS:
            raise Unsupported(f"mana cost.shards: unsupported shard {shard!r}")
        symbols += f"{{{COLORS[shard]}}}"
    return f'mana("{symbols}")' if symbols else "ManaCost::NONE"


def cost_parts(value: object) -> tuple[str, list[str]]:
    node = Node(value, "cost")
    kind = node.take("type")
    mana = "ManaCost::NONE"
    parts: list[str] = []
    if kind == "Mana":
        mana = mana_cost(node.take("cost"))
    elif kind == "Tap":
        parts = ["CostPart::TAP"]
    elif kind == "Sacrifice":
        node.expect("target", {"type": "SelfRef"})
        node.expect("count", 1)
        parts = ["CostPart::SACRIFICE"]
    elif kind == "Composite":
        for entry in items(node.take("costs")):
            child_mana, child_parts = cost_parts(entry)
            if child_mana != "ManaCost::NONE":
                if mana != "ManaCost::NONE":
                    raise Unsupported("cost: multiple mana components")
                mana = child_mana
            parts.extend(child_parts)
    else:
        raise Unsupported(
            f"cost.type: unsupported value {kind!r}", node_kind("cost", kind)
        )
    node.finish()
    return mana, parts


def cost(value: object) -> str:
    mana, parts = cost_parts(value)
    if not parts:
        return f"Cost::mana({mana})"
    if mana != "ManaCost::NONE":
        return f"Cost::of({mana}, &[{', '.join(parts)}])"
    if parts == ["CostPart::TAP"]:
        return "Cost::TAP"
    if parts == ["CostPart::TAP", "CostPart::SACRIFICE"]:
        return "tap_and_sacrifice()"
    return f"Cost::of_parts(&[{', '.join(parts)}])"


def mana_produced(value: object) -> str:
    node = Node(value, "mana produced")
    kind = node.take("type")
    if kind == "Fixed":
        colors = items(node.take("colors"))
        if (
            len(colors) != 1
            or not isinstance(colors[0], str)
            or colors[0] not in COLORS
        ):
            raise Unsupported("mana produced.colors: expected one mana of one color")
        result = f"ManaSet::one(ManaColor::{colors[0]})"
    elif kind == "Colorless" and fixed(node.take("count")) == 1:
        result = "ManaSet::one(ManaColor::Colorless)"
    elif kind == "AnyOneColor":
        count = fixed(node.take("count"))
        options = items(node.take("color_options"))
        if not options or any(
            not isinstance(c, str) or c not in COLORS for c in options
        ):
            raise Unsupported("mana produced.color_options: unknown mana symbol")
        mask = sum(1 << bit for bit, color in enumerate(COLORS) if color in options)
        result = (
            f"ManaSet::any_color({count})"
            if mask == 31
            else f"ManaSet {{ colors: {mask:#08b}, amount: {count} }}"
        )
    else:
        raise Unsupported(
            f"mana produced.type: unsupported value {kind!r}",
            node_kind("mana_produced", kind),
        )
    node.finish()
    return result


PLAYER_COUNTS = {
    "Draw": ("draw", "count"),
    "Discard": ("discard", "count"),
    "Mill": ("mill", "count"),
    "Scry": ("scry", "count"),
    "GainLife": ("gain_life", "amount"),
    "LoseLife": ("lose_life", "amount"),
}
OBJECT_EFFECTS = {
    "Bounce": "return_to_hand",
    "Destroy": "destroy",
    "Counter": "counter",
    "Regenerate": "regenerate",
    "PhaseOut": "phase_out",
    "Explore": "explore",
}


EFFECT_KINDS: set[str] = (
    PLAYER_COUNTS.keys()
    | OBJECT_EFFECTS.keys()
    | {
        "DealDamage",
        "Pump",
        "PutCounter",
        "ChangeZone",
    }
)


def effect(
    code: Node,
    duration: object,
    up_to: int = 0,
    keyword: str = "",
    *,
    all_players: bool = False,
) -> tuple[str, str]:
    kind = code.take("type")
    if not isinstance(kind, str):
        raise Unsupported(f"effect.type: unsupported value {kind!r}")
    if kind not in EFFECT_KINDS:
        raise Unsupported(f"effect.type: unsupported {kind}", node_kind("effect", kind))
    if up_to and kind != "Pump":
        raise Unsupported(f"ability.multi_target: unsupported on {kind}")
    if all_players:
        code.expect("target", None)
        affected = Target(players="each_player()")
    else:
        affected = target(code.take("target"), up_to)
    if kind not in ("Pump", "ChangeZone") and duration is not None:
        raise Unsupported(f"ability.duration: unsupported on {kind}")
    if kind in PLAYER_COUNTS and affected.players:
        builder, amount = PLAYER_COUNTS[kind]
        if kind == "Mill":
            code.expect("destination", "Graveyard")
        if kind in ("GainLife", "LoseLife") and not (affected.spec or all_players):
            code.expect("player", "controller")
        result = f"{builder}({affected.players}, {fixed(code.take(amount))})"
    elif kind in OBJECT_EFFECTS and affected.objects:
        builder = OBJECT_EFFECTS[kind]
        if kind == "Destroy":
            no_regeneration = code.take("cant_regenerate", False)
            if type(no_regeneration) is not bool:
                raise Unsupported("effect.cant_regenerate: expected a boolean")
            if no_regeneration:
                builder = "destroy_without_regeneration"
        if kind == "Counter":
            if affected.kind != "Spell":
                raise Unsupported("effect.target: expected a spell")
            code.expect("source_static", None)
            code.expect("unless_payment", None)
        if kind == "Bounce":
            code.expect("destination", None)
        result = f"{builder}({affected.objects})"
    elif kind == "DealDamage" and affected.spec:
        amount = fixed(code.take("amount"))
        if affected.kind == "Any":
            result = f"deal_damage_to_any_target({amount})"
        elif affected.players:
            result = f"deal_damage_to({affected.players}, {amount})"
        elif affected.kind == "Permanent":
            result = (
                f"Effect::DealDamage {{ to: Recipients::Permanents({affected.objects}), "
                f"amount: Amount::Fixed({amount}) }}"
            )
        else:
            raise Unsupported("effect.target: expected a damage recipient")
    elif kind == "Pump" and affected.objects and duration == "UntilEndOfTurn":
        power = fixed(code.take("power"), -128, 127)
        toughness = fixed(code.take("toughness"), -128, 127)
        modifications = f"Modification::ModifyPt({power}, {toughness})"
        if keyword:
            modifications += f", Modification::AddKeywords(Keywords::{keyword})"
        result = f"modify({affected.objects}, &[{modifications}], Duration::EndOfTurn)"
    elif kind == "PutCounter" and affected.objects:
        counter_kind = code.take("counter_type")
        if counter_kind not in ("P1P1", "M1M1"):
            raise Unsupported(
                f"effect.counter_type: unsupported value {counter_kind!r}"
            )
        counter = "PlusOne" if counter_kind == "P1P1" else "MinusOne"
        result = f"add_counters({affected.objects}, CounterKind::{counter}, {fixed(code.take('count'))})"
    elif kind == "ChangeZone" and affected.objects:
        result = change_zone(code, affected.objects, duration)
    else:
        raise Unsupported(
            f"effect.type: unsupported {kind} with this target or duration"
        )
    code.finish()
    return result, affected.spec


def change_zone(node: Node, objects: str, duration: object) -> str:
    node.expect("origin", None)
    for flag in (
        "owner_library",
        "enter_transformed",
        "under_your_control",
        "enter_tapped",
        "enters_attacking",
    ):
        node.expect(flag, False)
    destination = node.take("destination")
    if destination == "Exile" and duration == "UntilHostLeavesPlay":
        return f"exile_until_this_leaves({objects})"
    if destination == "Exile" and duration is None:
        return f"exile({objects})"
    raise Unsupported(
        f"effect.destination: unsupported {destination!r} with duration {duration!r}"
    )


@dataclass(frozen=True)
class Ability:
    """One ability's builder and effects, which can be combined for a spell."""

    builder: str
    effects: tuple[str, ...] = ()
    target: str = ""

    def expression(self) -> str:
        builder = self.builder
        if self.target:
            builder += f".target(&{self.target})"
        if not self.effects:
            return builder
        result = self.effects[0]
        if len(self.effects) > 1:
            result = f"sequence(&[{', '.join(self.effects)}])"
        if not self.target and builder == "Ability::triggered(&Trigger::ENTERS)":
            return f"when_enters(&{result})"
        if not self.target and builder == "Ability::triggered(&Trigger::ATTACKS)":
            return f"whenever_attacks(&{result})"
        return f"{builder}.effect(&{result})"


def optional_target_limit(value: object) -> int:
    node = Node(value, "ability.multi_target")
    minimum = node.take("min")
    maximum = node.take("max")
    if (
        type(minimum) is not int
        or minimum != 0
        or type(maximum) is not int
        or maximum != 2
    ):
        raise Unsupported("ability.multi_target: expected a range of zero to two")
    node.finish()
    return maximum


PUMP_KEYWORDS = {"FirstStrike": "FIRST_STRIKE", "Haste": "HASTE", "Trample": "TRAMPLE"}


def pump_keyword(value: object) -> str:
    """Read the keyword clause that shares the pump's targets and duration."""
    node = Node(value, "pump sub_ability")
    node.expect("kind", "Spell", required=True)
    node.expect("duration", "UntilEndOfTurn", required=True)
    body = Node(node.take("effect"), "pump sub_ability.effect")
    for key in ("cost", "sub_ability", "target_prompt", "condition"):
        node.expect(key, None)
    for key in ("sorcery_speed", "optional_targeting", "optional", "forward_result"):
        node.expect(key, False)
    node.take("description")
    node.finish()

    body.expect("type", "GenericEffect", required=True)
    body.expect("target", None)
    body.expect("duration", "UntilEndOfTurn", required=True)
    statics = items(body.take("static_abilities"))
    body.finish()
    if len(statics) != 1:
        raise Unsupported("pump sub_ability.effect: expected one keyword grant")

    grant = Node(statics[0], "pump keyword")
    grant.expect("mode", "Continuous", required=True)
    # Phase uses SelfRef for the subject of the joined "get ... and gain ..." clause.
    grant.expect("affected", {"type": "SelfRef"}, required=True)
    for key in ("condition", "affected_zone", "effect_zone"):
        grant.expect(key, None)
    grant.expect("active_zones", [])
    grant.expect("characteristic_defining", False)
    grant.take("description")
    modifications = items(grant.take("modifications"))
    grant.finish()
    if len(modifications) != 1:
        raise Unsupported("pump keyword: expected one modification")
    modification = Node(modifications[0], "pump keyword modification")
    modification.expect("type", "AddKeyword", required=True)
    keyword = modification.take("keyword")
    modification.finish()
    if not isinstance(keyword, str) or keyword not in PUMP_KEYWORDS:
        raise Unsupported(f"pump keyword: unsupported {keyword!r}")
    return PUMP_KEYWORDS[keyword]


def ability(value: object, trigger: str = "", parent: bool = False) -> Ability:
    node = Node(value, "ability")
    kind = node.take("kind")
    activation = node.take("cost")
    duration = node.take("duration")
    child = node.take("sub_ability")
    multi_target = node.take("multi_target")
    up_to = 0
    if multi_target is not None:
        if kind != "Spell" or trigger or parent:
            raise Unsupported("ability.multi_target: expected a top-level spell")
        up_to = optional_target_limit(multi_target)
    body = Node(node.take("effect"), "effect")
    player_scope = node.take("player_scope")
    # A Phase sub-ability can depend on the enclosing player scope.
    if player_scope is not None and (
        player_scope != {"type": "All"}
        or body.fields.get("type") != "LoseLife"
        or child is not None
    ):
        raise Unsupported(
            f"ability.player_scope: unsupported value {player_scope!r} for this ability"
        )
    description = node.take("description")
    node.expect("condition", None)
    node.expect("target_prompt", None)
    node.expect("optional_targeting", False)
    node.expect("optional", False)
    node.expect("forward_result", False)
    sorcery_speed = node.take("sorcery_speed", False)
    node.finish()
    if kind == "Activated" and not parent and not trigger:
        payment = cost(activation)
        if body.fields.get("type") == "Mana":
            if child is not None or duration is not None or sorcery_speed:
                raise Unsupported(
                    "mana ability has a sub_ability, duration or timing limit"
                )
            body.expect("type", "Mana")
            produced = mana_produced(body.take("produced"))
            body.finish()
            if payment == "Cost::TAP":
                return Ability(f"Ability::tap_for_mana({produced})")
            return Ability(f"Ability::mana({payment}, {produced})")
        builder = f"Ability::activated({payment})"
        if sorcery_speed is True:
            builder += ".as_a_sorcery()"
        elif sorcery_speed is not False:
            raise Unsupported("ability.sorcery_speed: expected a boolean")
    elif (
        (kind == "Spell" or (kind == "Activated" and parent))
        and activation is None
        and sorcery_speed is False
    ):
        builder = trigger or "Ability::spell()"
    else:
        raise Unsupported(
            f"ability.kind: unsupported {kind!r} with this cost or timing"
        )
    keyword = ""
    if (
        up_to
        and child is not None
        and duration is None
        and body.fields.get("type") == "Pump"
    ):
        keyword = pump_keyword(child)
        duration = "UntilEndOfTurn"
        child = None
    expression, spec = effect(
        body, duration, up_to, keyword, all_players=player_scope is not None
    )
    if child is None:
        drafted = Ability(builder, (expression,), spec)
    else:
        following = ability(child, parent=True)
        if spec and following.target:
            raise Unsupported("sub_ability adds a second target")
        drafted = Ability(
            builder, (expression, *following.effects), spec or following.target
        )
    # Phase sometimes parses "Target player draws two cards" as a Draw with no target, which
    # would draft as "draw two cards". Its sentence still says "target", so stop instead.
    if not parent and not drafted.target and TARGET_WORD.search(str(description or "")):
        raise Unsupported(
            "ability.effect: the sentence names a target the parse dropped"
        )
    return drafted


TARGET_WORD: re.Pattern[str] = re.compile(r"\btarget\b", re.IGNORECASE)


TRIGGERS: dict[tuple[str, str | None, str | None], str] = {
    ("ChangesZone", None, "Battlefield"): "ENTERS",
    ("ChangesZone", "Battlefield", "Graveyard"): "DIES",
    ("LeavesBattlefield", None, None): "LEAVES",
    ("Attacks", None, None): "ATTACKS",
}


def triggered(value: object) -> Ability:
    node = Node(value, "trigger")
    mode = node.take("mode")
    if all(mode != supported for supported, _, _ in TRIGGERS):
        raise Unsupported(
            f"trigger.mode: unsupported {mode!r}", node_kind("trigger", mode)
        )
    valid_card = node.take("valid_card")
    origin = node.take("origin")
    destination = node.take("destination")
    node.take("description")
    node.expect("trigger_zones", ["Battlefield"])
    node.expect("phase", None)
    node.expect("optional", False)
    node.expect("damage_kind", "Any")
    node.expect("secondary", False)
    node.expect("valid_target", None)
    node.expect("valid_source", None)
    node.expect("constraint", None)
    node.expect("condition", None)
    node.expect("batched", False)
    builder = ""
    if valid_card == {"type": "SelfRef"}:
        builder = TRIGGERS.get(
            (
                str(mode),
                str(origin) if origin is not None else None,
                str(destination) if destination is not None else None,
            ),
            "",
        )
    if not builder:
        raise Unsupported(
            f"trigger.mode: unsupported {mode!r} for this subject and zones"
        )
    result = ability(node.take("execute"), f"Ability::triggered(&Trigger::{builder})")
    node.finish()
    return result


def replacement(value: object) -> Ability:
    node = Node(value, "replacement")
    event = node.take("event", "Moved")
    if event != "Moved":
        raise Unsupported(
            f"replacement.event: unsupported {event!r}", node_kind("replacement", event)
        )
    node.expect("mode", {"type": "Mandatory"})
    node.expect("valid_card", {"type": "SelfRef"})
    node.expect("condition", None)
    node.take("description")
    execute = Node(node.take("execute"), "replacement.execute")
    execute.expect("kind", "Spell")
    execute.expect("effect", {"type": "Tap", "target": {"type": "SelfRef"}})
    for key in (
        "cost",
        "sub_ability",
        "duration",
        "description",
        "target_prompt",
        "condition",
    ):
        execute.expect(key, None)
    for key in ("sorcery_speed", "optional_targeting", "optional", "forward_result"):
        execute.expect(key, False)
    execute.finish()
    node.finish()
    return Ability("Ability::replacing(&Replacement::ENTERS_TAPPED)")
