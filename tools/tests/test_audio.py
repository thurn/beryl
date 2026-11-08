import json
from pathlib import Path

from beryl_tools import audio


def test_committed_audio_matches_the_manifest() -> None:
    # Every cue the UI can ask for has its file, none is left over, the loop lengths cover
    # the looping music, and the whole fits the budget.
    assert audio.problems(audio.load()) == []


def test_problems_name_missing_stray_and_unlooped(tmp_path: Path) -> None:
    manifest = audio.load()
    out = tmp_path / "audio"
    names = list(audio.outputs(manifest))
    for name in names[1:]:
        (out / name).parent.mkdir(parents=True, exist_ok=True)
        (out / name).write_bytes(b"")
    (out / "sfx" / "old-0.wav").write_bytes(b"")
    loops = tmp_path / "loops.json"
    loops.write_text(json.dumps({"menu": [1.0]}))
    assert audio.problems(manifest, out, loops) == [
        f"missing {names[0]}",
        "loops.json does not match the looping music; run `just audio`",
        "stray sfx/old-0.wav",
    ]
