"""`by card-quality`: every cube card's games-in-hand win rate from 17lands' public game
data, into data/card-quality.csv, the card knowledge search may use without hand-tuning.

A card's games in hand are the games where it was in the opening hand, drawn or tutored;
its win rate there (GIH WR) is 17lands' standard measure of how much a card helps the deck
that plays it. The improvement when drawn (IWD) is GIH WR less the win rate of the games
where the card was in the deck but never seen, which takes out most of how strong the decks
that play it are. PremierDraft and TradDraft are pooled."""

from __future__ import annotations

import argparse
import csv
import gzip
import sys
from dataclasses import dataclass
from pathlib import Path

from .paths import REPO

DATA: Path = Path.home() / "17lands" / "Cube_-_Powered"
OUTPUT: Path = REPO / "data" / "card-quality.csv"
EVENTS = ("PremierDraft", "TradDraft")
SEEN = ("opening_hand_", "drawn_", "tutored_")


@dataclass
class Tally:
    games: int = 0
    wins: int = 0
    unseen: int = 0
    unseen_wins: int = 0


def tally(rows: csv.DictReader[str], counts: dict[str, Tally]) -> None:
    """Adds each row's game to the tally of every card in its deck: seen in hand, or not."""
    fields = rows.fieldnames or []
    names = sorted({f[len("deck_") :] for f in fields if f.startswith("deck_")})
    columns = {n: [p + n for p in SEEN if p + n in fields] for n in names}
    for row in rows:
        won = row["won"] == "True"
        for name, cols in columns.items():
            if any(row[c] not in ("", "0") for c in cols):
                t = counts.setdefault(name, Tally())
                t.games += 1
                t.wins += won
            elif row["deck_" + name] not in ("", "0"):
                t = counts.setdefault(name, Tally())
                t.unseen += 1
                t.unseen_wins += won


def read(data: Path) -> dict[str, Tally]:
    counts: dict[str, Tally] = {}
    for event in EVENTS:
        path = data / f"game_data_public.Cube_-_Powered.{event}.csv.gz"
        with gzip.open(path, "rt", newline="") as f:
            tally(csv.DictReader(f), counts)
    return counts


def write(counts: dict[str, Tally], out: Path) -> None:
    with out.open("w", newline="") as f:
        w = csv.writer(f, lineterminator="\n")
        w.writerow(["name", "games_in_hand", "gih_wr", "iwd"])
        for name in sorted(counts):
            t = counts[name]
            if t.games and t.unseen:
                gih = t.wins / t.games
                iwd = gih - t.unseen_wins / t.unseen
                w.writerow([name, t.games, f"{gih:.4f}", f"{iwd:+.4f}"])


def card_quality(args: argparse.Namespace) -> int:
    counts = read(Path(args.data))
    write(counts, Path(args.output))
    print(f"{len(counts)} cards -> {args.output}", file=sys.stderr)
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "card-quality",
        help="17lands games-in-hand win rates into data/card-quality.csv",
    )
    p.add_argument("--data", default=str(DATA))
    p.add_argument("--output", default=str(OUTPUT))
    p.set_defaults(func=card_quality)
