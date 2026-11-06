import csv
import gzip
from pathlib import Path

from beryl_tools.cli import main

HEADER = [
    "won",
    "opening_hand_A",
    "drawn_A",
    "tutored_A",
    "deck_A",
    "opening_hand_B, Two",
    "drawn_B, Two",
    "tutored_B, Two",
    "deck_B, Two",
]


def test_win_rates_in_hand_and_unseen(tmp_path: Path) -> None:
    # A is seen in two games, won one; in the deck unseen in one, lost. B, whose name has a
    # comma, is seen in one won game and unseen in two, one won.
    rows = [
        ["True", "1", "0", "0", "1", "0", "1", "0", "1"],
        ["False", "0", "1", "0", "1", "0", "0", "0", "1"],
        ["False", "0", "0", "0", "1", "0", "0", "0", "0"],
        ["True", "0", "0", "0", "0", "0", "0", "0", "1"],
    ]
    for event in ("PremierDraft", "TradDraft"):
        path = tmp_path / f"game_data_public.Cube_-_Powered.{event}.csv.gz"
        with gzip.open(path, "wt", newline="") as f:
            w = csv.writer(f)
            w.writerow(HEADER)
            w.writerows(rows if event == "PremierDraft" else [])
    out = tmp_path / "quality.csv"
    assert main(["card-quality", "--data", str(tmp_path), "--output", str(out)]) == 0
    got = list(csv.reader(out.open()))
    assert got == [
        ["name", "games_in_hand", "gih_wr", "iwd"],
        ["A", "2", "0.5000", "+0.5000"],
        ["B, Two", "1", "1.0000", "+0.5000"],
    ]
