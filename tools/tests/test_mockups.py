import argparse
import json
import struct

from beryl_tools import mockups, shots


def png_size(path: str) -> tuple[int, int]:
    with open(path, "rb") as f:
        header = f.read(24)
    width, height = struct.unpack(">II", header[16:24])
    return width, height


def test_regions_lie_inside_their_mockups() -> None:
    found = json.loads(shots.REGIONS.read_text())["mockups"]
    assert set(found) == {
        "battlefield",
        "combat",
        "scry",
        "trigger_on_stack_with_target",
        "ui_system",
    }
    for name, m in found.items():
        width, height = png_size(str(shots.REGIONS.parent / m["file"]))
        rects = {f"mask {i}": r for i, r in enumerate(m["masks"])}
        for part, spec in m["parts"].items():
            rects[part] = spec if isinstance(spec, list) else spec["rect"]
            for i, r in enumerate([] if isinstance(spec, list) else spec["masks"]):
                rects[f"{part} mask {i}"] = r
        for slot, spec in m["art"].items():
            rects[f"art {slot}"] = spec if isinstance(spec, list) else spec["rect"]
        for label, (x, y, w, h) in rects.items():
            assert w > 0 and h > 0, f"{name} {label}"
            assert 0 <= x and x + w <= width, f"{name} {label}"
            assert 0 <= y and y + h <= height, f"{name} {label}"


def test_compare_refuses_globs_matching_no_part() -> None:
    args = argparse.Namespace(parts=["nowhere/*"], zoom=1)
    assert mockups.ui_compare(args) == 2
