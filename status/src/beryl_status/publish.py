"""`just status publish`: the built site on GitHub Pages, from the `gh-pages` branch.

Each publish is one commit with no parent, force-pushed, so the branch never grows a history
of site builds. The commit is made with a separate index file in this cache, never a checkout,
so the main checkout is not touched; the index remembers each file's stat information, so
hashing only reads the files a build rewrote. Git sends only the objects the remote lacks:
an unchanged page costs nothing.

The beads are already public, in the repository's `refs/dolt/data`, so the site publishes
nothing new. GitHub Pages builds at most about ten times an hour from a branch, which is why
the launchd agent publishes at most every ten minutes, and a publish whose tree is unchanged
pushes nothing."""

from __future__ import annotations

import json
import os
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path

from cirrus.assets import Assets

from . import paths, render

BRANCH = "gh-pages"
REMOTE = "origin"
GITHUB: re.Pattern[str] = re.compile(r"github\.com[:/]([^/]+)/([^/]+?)(?:\.git)?/?$")


@dataclass(frozen=True)
class Repository:
    owner: str
    name: str

    @property
    def pages_url(self) -> str:
        return f"https://{self.owner}.github.io/{self.name}/"

    @property
    def base(self) -> str:
        """The site's root path on GitHub Pages, a project site under the repository's name."""
        return f"/{self.name}/"


def git(
    main: Path, *args: str, env: dict[str, str] | None = None, data: str = ""
) -> str:
    done = subprocess.run(
        ["git", *args],
        cwd=main,
        input=data,
        capture_output=True,
        text=True,
        env={**os.environ, **(env or {})},
        check=False,
    )
    if done.returncode:
        raise RuntimeError(f"git {' '.join(args)}: {done.stderr.strip()}")
    return done.stdout.strip()


def repository(main: Path) -> Repository:
    url = git(main, "remote", "get-url", REMOTE)
    found = GITHUB.search(url)
    if found is None:
        raise RuntimeError(f"{REMOTE} is not a GitHub repository: {url}")
    return Repository(found.group(1), found.group(2))


def state_file() -> Path:
    return paths.folder() / "published.json"


def last_tree() -> str:
    try:
        raw = json.loads(state_file().read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return ""
    tree = raw.get("tree") if isinstance(raw, dict) else None
    return tree if isinstance(tree, str) else ""


def tree_of(main: Path, site: Path, repo: Repository) -> str:
    """The tree of the site as GitHub Pages serves it: the built files, with the 404 page's
    links resolved against the Pages project's root instead of `/`."""
    index_file = str(paths.folder() / "publish.index")
    env = {"GIT_INDEX_FILE": index_file, "GIT_WORK_TREE": str(site)}
    git(main, "add", "--all", "--force", ".", env={**env, "GIT_DIR": git_dir(main)})
    missing = render.not_found(Assets.load(), repo.base)
    blob = git(main, "hash-object", "-w", "--stdin", data=missing)
    git(
        main,
        "update-index",
        "--cacheinfo",
        f"100644,{blob},404.html",
        env={**env, "GIT_DIR": git_dir(main)},
    )
    return git(main, "write-tree", env={**env, "GIT_DIR": git_dir(main)})


def git_dir(main: Path) -> str:
    return git(main, "rev-parse", "--path-format=absolute", "--git-common-dir")


def ensure_pages(repo: Repository) -> None:
    """Turns GitHub Pages on for the branch, once: a repository with Pages already on answers
    the first request."""
    endpoint = f"repos/{repo.owner}/{repo.name}/pages"
    found = subprocess.run(["gh", "api", endpoint], capture_output=True, text=True)
    if found.returncode == 0:
        return
    made = subprocess.run(
        [
            "gh",
            "api",
            "-X",
            "POST",
            endpoint,
            "-f",
            f"source[branch]={BRANCH}",
            "-f",
            "source[path]=/",
        ],
        capture_output=True,
        text=True,
    )
    if made.returncode:
        raise RuntimeError(f"turning on GitHub Pages: {made.stderr.strip()}")


def publish(main: Path, site: Path) -> str:
    """Pushes the site to `gh-pages` unless its tree is the one last pushed; what it did."""
    repo = repository(main)
    tree = tree_of(main, site, repo)
    if tree == last_tree():
        return f"status: unchanged since the last publish; {repo.pages_url}"
    commit = git(main, "commit-tree", tree, "-m", "Publish the bead status site")
    push = ("push", "--force", "--quiet", REMOTE, f"{commit}:refs/heads/{BRANCH}")
    try:
        git(main, *push)
    except RuntimeError:
        # GitHub sometimes rejects a large push with no reason ("remote rejected ...
        # (failed)"), and the same push succeeds when sent again.
        git(main, *push)
    ensure_pages(repo)
    state_file().write_text(json.dumps({"tree": tree, "commit": commit}) + "\n")
    return f"status: published {commit[:9]} to {BRANCH}; {repo.pages_url}"
