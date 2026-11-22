"""File changes planned by Beryl 3D scaffolds."""

from __future__ import annotations

import re
from pathlib import Path

CRATES = Path("rust/beryl3d/crates")


class Invalid(ValueError):
    """The requested scaffold cannot be applied to this checkout."""


class Files:
    """Plan and validate the entire change before writing files."""

    def __init__(self, root: Path, name: str) -> None:
        self.root = root
        self.name = name
        self.module: str = name.replace("-", "_")
        self.variant: str = "".join(part.capitalize() for part in name.split("-"))
        self.original: dict[Path, str | None] = {}
        self.changed: dict[Path, str] = {}
        self.story: str = ""

    def read(self, path: Path) -> str:
        if path in self.changed:
            return self.changed[path]
        return (self.root / path).read_text()

    def add(self, path: Path, text: str) -> None:
        target = self.root / path
        if target.exists() or target.is_symlink() or path in self.changed:
            raise Invalid(f"{path} already exists")
        self.original[path] = None
        self.changed[path] = text

    def edit(self, path: Path, text: str) -> None:
        if path not in self.original:
            self.original[path] = (self.root / path).read_text()
        self.changed[path] = text

    def replace(self, path: Path, anchor: str, replacement: str) -> None:
        source = self.read(path)
        if source.count(anchor) != 1:
            raise Invalid(f"{path}: expected one occurrence of {anchor.strip()!r}")
        self.edit(path, source.replace(anchor, replacement))

    def insert(self, path: Path, anchor: str, text: str) -> None:
        self.replace(path, anchor, text + anchor)

    def append(self, path: Path, text: str) -> None:
        self.edit(path, self.read(path) + text)

    def require_new_module(self, directory: Path) -> None:
        module = directory / self.module
        for path in (module, module.with_suffix(".rs")):
            target = self.root / path
            if target.exists() or target.is_symlink():
                raise Invalid(f"{path} already exists")

    def require_new_word(self, path: Path, *words: str) -> None:
        source = self.read(path)
        for word in words:
            if re.search(rf"(?<![\w-]){re.escape(word)}(?![\w-])", source):
                raise Invalid(f"{word!r} already exists in {path}")

    def story_file(
        self, crate: str, name: str, text: str, folder: Path | None = None
    ) -> None:
        base = CRATES / crate
        for existing in (self.root / base).rglob(f"{name}.story"):
            if "stories" in existing.relative_to(self.root / base).parts[:-1]:
                raise Invalid(
                    f"{existing.relative_to(self.root)} already uses story/{crate}/{name}"
                )
        directory = folder if folder is not None else base / "stories"
        self.add(directory / f"{name}.story", text)
        self.story = f"story/{crate}/{name}"

    def test(self, crate: str, name: str, source: str) -> None:
        base = CRATES / crate / "tests/all"
        self.add(base / f"{name}.rs", source)
        self.append(base / "main.rs", f"\nmod {name};\n")

    def write(self) -> None:
        applied: list[Path] = []
        for path, previous in self.original.items():
            target = self.root / path
            if previous is None:
                if target.exists() or target.is_symlink():
                    raise Invalid(f"{path} already exists")
            elif target.read_text() != previous:
                raise Invalid(f"{path} changed while preparing the scaffold")
        try:
            # Reserve all new paths before making any changes to shared registries.
            for path, previous in self.original.items():
                if previous is not None:
                    continue
                target = self.root / path
                target.parent.mkdir(parents=True, exist_ok=True)
                with target.open("x") as output:
                    applied.append(path)
                    output.write(self.changed[path])
            for path, previous in self.original.items():
                if previous is not None:
                    applied.append(path)
                    (self.root / path).write_text(self.changed[path])
        except OSError:
            for path in reversed(applied):
                target = self.root / path
                previous = self.original[path]
                if previous is None:
                    target.unlink()
                else:
                    target.write_text(previous)
            raise
