"""The model trial tool, on made-up ticket JSON. No test calls `ignite`, git or the efficiency
store."""

from __future__ import annotations

import json
from typing import Any

import pytest

from beryl_tools.model_trial import candidates, copytext, grade, manifest, pair, scan
from beryl_tools.model_trial.manifest import FILED, LISTED, MARKED, Pair
from beryl_tools.model_trial.scan import Trace
from beryl_tools.model_trial.spec import Spec, SpecError, from_notes, parse

SPEC_JSON: dict[str, Any] = {
    "arms": {
        "source": {"tier": "opus", "provider": "claude", "effort": "medium"},
        "copy": {"tier": "sonnet", "provider": "claude", "effort": "high"},
    },
    "block_size": 6,
    "max_pairs": 24,
    "cap_usd": 260,
    "tickets": ["E1.1", "E1.2", "E1.3"],
}
START = "a" * 40


def spec(**changes: Any) -> Spec:
    return parse({**SPEC_JSON, **changes})


def ticket(tid: str = "E1.1", **fields: Any) -> dict[str, Any]:
    base: dict[str, Any] = {
        "id": tid,
        "parent": "E1",
        "title": "Preserve linked exile across source reentry",
        "body": "Change `engine/crates/beryl_rules/src/objects/linked.rs`. The store keys links by slot.",
        "acceptance": "The old ability finds the old cards.",
        "notes": "",
        "type": "bug",
        "status": "open",
        "priority": 2,
        "assignee": None,
        "session": None,
        "labels": ["tier:opus"],
        "blocked_by": [],
        "allocation": None,
        "needs_os": None,
        "metadata": {"ignite_checks": ["rust"]},
        "estimate_minutes": 65,
        "version": 7,
    }
    return {**base, **fields}


def epic(**fields: Any) -> dict[str, Any]:
    return {
        "id": "E1",
        "parent": None,
        "type": "epic",
        "status": "open",
        "allocation": 0,
        "labels": [],
        "body": "The epic's plan.",
        "design": "Run every command through `ignite run --`.",
        "needs_os": None,
        **fields,
    }


def context(
    *tickets: dict[str, Any], workers: set[str] | None = None
) -> candidates.Context:
    known = {str(t["id"]): t for t in (epic(), *tickets)}
    return candidates.Context(spec(), known, workers or {"jarvis"}, set())


def filed_pair(**fields: Any) -> Pair:
    base: dict[str, Any] = {
        "pair": "c0ffee00",
        "trial": "T1",
        "source": "E1.1",
        "order": 0,
        "state": MARKED,
        "kind": "engine",
        "copy_title": "Keep exiled-card links when a source leaves and comes back",
    }
    return Pair(**{**base, **fields})


def filing(p: Pair, source: dict[str, Any], **kwargs: Any) -> pair.Filing:
    known = {str(t["id"]): t for t in (kwargs.pop("epic", epic()), source)}
    return pair.build_filing(
        p,
        spec(),
        source,
        known,
        START,
        kwargs.pop("siblings", {}),
        kwargs.pop("home", None),
    )


# The spec.


def test_a_missing_optional_field_gets_its_default() -> None:
    data = {k: v for k, v in SPEC_JSON.items() if k not in ("block_size", "max_pairs")}
    found = parse(data)
    assert found.block_size == 6
    assert found.pair_limit == 3
    assert found.reviewer.runs_per_pair == 2


@pytest.mark.parametrize(
    "change, field",
    [
        ({"cap_usd": None}, "cap_usd"),
        ({"block_size": 0}, "block_size"),
        ({"tickets": []}, "tickets"),
        ({"arms": {"source": SPEC_JSON["arms"]["source"]}}, "arms.copy"),
        (
            {
                "arms": {
                    **SPEC_JSON["arms"],
                    "source": {"tier": "opus", "provider": "claude"},
                }
            },
            "arms.source.effort",
        ),
    ],
)
def test_a_bad_spec_is_refused_naming_the_field(
    change: dict[str, Any], field: str
) -> None:
    data = {**SPEC_JSON, **change}
    data = {k: v for k, v in data.items() if v is not None}
    with pytest.raises(SpecError, match=field.replace(".", r"\.")):
        parse(data)


def test_the_spec_is_read_from_the_epics_notes() -> None:
    notes = f"## Trial spec\n\nIntro.\n\n```json\n{json.dumps(SPEC_JSON)}\n```\n\nLater text."
    assert from_notes(notes).tickets == ("E1.1", "E1.2", "E1.3")
    with pytest.raises(SpecError, match="heading"):
        from_notes("no spec here")


def test_two_trials_keep_their_records_apart() -> None:
    manifest.save_pair(filed_pair(trial="T1", state=LISTED))
    manifest.save_pair(
        filed_pair(trial="T2", pair="beef0000", source="E9.1", state=LISTED)
    )
    assert [p.source for p in manifest.read_pairs("T1")] == ["E1.1"]
    assert [p.source for p in manifest.read_pairs("T2")] == ["E9.1"]
    assert manifest.trial_dir("T1") != manifest.trial_dir("T2")


def test_a_pairs_latest_row_is_its_state() -> None:
    row = filed_pair(state=LISTED)
    manifest.save_pair(row)
    row.state = MARKED
    manifest.save_pair(row)
    assert [p.state for p in manifest.read_pairs("T1")] == [MARKED]


# Eligibility and kinds.


@pytest.mark.parametrize(
    "fields, reason",
    [
        ({"status": "closed"}, "not open"),
        ({"assignee": "cli-claude-1234abcd"}, "claimed"),
        ({"blocked_by": [{"id": "E1.9", "rule": 2}]}, "waiting on E1.9"),
        ({"labels": ["tier:sonnet"]}, "tier:sonnet, not tier:opus"),
        ({"labels": []}, "no tier label"),
        ({"labels": ["tier:opus", "provider:codex"]}, "provider:codex conflicts"),
        ({"labels": ["tier:opus", "effort:high"]}, "effort:high conflicts"),
        ({"labels": ["tier:opus", "review"]}, "a review"),
        ({"body": "**Lead.** Followers: E1.4."}, "a lead"),
        (
            {"labels": ["tier:opus", "machine:multivac"]},
            "machine:multivac, which has no worker",
        ),
        ({"labels": ["tier:opus", "trial:copy"]}, "already in a trial"),
        (
            {"metadata": {"saved_branch": "refs/ignite/tickets/E1.1"}},
            "has a saved branch",
        ),
    ],
)
def test_each_exclusion_reason(fields: dict[str, Any], reason: str) -> None:
    t = ticket(**fields)
    found = candidates.exclusion(t, context(t))
    assert found is not None and reason in found


def test_an_eligible_ticket_has_no_reason() -> None:
    t = ticket(status="pending_approval", blocked_by=[{"id": "E1.1", "rule": 1}])
    assert candidates.exclusion(t, context(t)) is None


def test_a_hub_ticket_with_no_tier_is_eligible() -> None:
    t = ticket("AJNHT", parent=None, labels=["hub"])
    assert candidates.exclusion(t, context(t)) is None


def test_a_ticket_already_in_a_trial_is_left_out() -> None:
    t = ticket()
    ctx = context(t)
    ctx.trialed = {"E1.1"}
    assert candidates.exclusion(t, ctx) == "already in a trial"


def test_a_machine_pin_is_inherited_from_the_epic() -> None:
    t = ticket()
    ctx = context(t, workers={"cygnus"})
    ctx.tickets["E1"] = epic(labels=["machine:jarvis"])
    found = candidates.exclusion(t, ctx)
    assert found is not None and "machine:jarvis" in found


@pytest.mark.parametrize(
    "text, kind",
    [
        (
            "`engine/cards/m10/bolt.rs` and `engine/crates/beryl_rules/src/a.rs`",
            "engine",
        ),
        (
            "`client/src/scene.rs`, `engine/crates/beryl_app/src/x.rs`, `tools/x.py`",
            "interface",
        ),
        (
            "`tools/a.py` and `docs/b.md` and `engine/crates/beryl_rules/x.rs`",
            "tooling",
        ),
        ("`beryl_rules/src/linked.rs` and `docs/plan.md`", "engine"),
        ("No paths at all.", "other"),
    ],
)
def test_kinds(text: str, kind: str) -> None:
    assert candidates.kind_of(text) == kind


# Blocks, plants and titles.


def test_pair_next_takes_listed_tickets_in_order() -> None:
    pairs = [
        filed_pair(pair=f"p{i}", source=f"E1.{i}", order=i, state=LISTED)
        for i in (3, 1, 2)
    ]
    pairs.append(filed_pair(pair="p0", source="E1.0", order=0, state=FILED))
    marked, skipped = pair.mark_block(pairs, block=1, count=2, limit=24)
    assert [p.source for p in marked] == ["E1.1", "E1.2"]
    assert all(p.block == 1 and p.state == MARKED for p in marked)
    assert skipped == []


def test_pair_next_skips_a_ticket_that_can_no_longer_be_an_arm() -> None:
    pairs = [
        filed_pair(pair=f"p{i}", source=f"E1.{i}", order=i, state=LISTED)
        for i in range(4)
    ]
    marked, skipped = pair.mark_block(
        pairs,
        block=1,
        count=2,
        limit=24,
        excluded=lambda p: "has a saved branch" if p.source == "E1.1" else None,
    )
    assert [p.source for p in marked] == ["E1.0", "E1.2"]
    assert [(p.source, p.state) for p in skipped] == [("E1.1", manifest.SKIPPED)]
    note = skipped[0].note
    assert note is not None and "saved branch" in note
    assert pairs[3].state == LISTED


def test_a_skipped_source_gets_its_allocation_back() -> None:
    p = filed_pair(state=manifest.SKIPPED, previous_allocation=50)
    assert pair.unpark(p) == {"op": "update", "id": "E1.1", "set": {"allocation": 50}}


def test_pair_next_never_passes_the_trials_limit() -> None:
    pairs = [
        filed_pair(pair=f"p{i}", source=f"E1.{i}", order=i, state=LISTED)
        for i in range(5)
    ]
    pairs[0].state = FILED
    assert len(pair.mark_block(pairs, block=1, count=6, limit=3)[0]) == 2


def test_a_planted_sentence_must_appear_exactly_once() -> None:
    p = filed_pair()
    with pytest.raises(pair.PairError, match="0 times"):
        pair.plant(p, ticket(), "Not in the text.", "False.", "Notices.")
    pair.plant(
        p,
        ticket(),
        "The store keys links by slot.",
        "The store keys links by name.",
        "x",
    )
    assert p.plant is not None


def test_a_copy_title_must_not_give_the_trial_away() -> None:
    source = ticket()
    with pytest.raises(pair.PairError, match="source's title"):
        pair.check_title(str(source["title"]), source)
    with pytest.raises(pair.PairError, match="copy"):
        pair.check_title("A copy of the exile fix", source)
    pair.check_title(
        "Keep exiled-card links when a source leaves and comes back", source
    )


def test_an_audit_needs_a_plant_and_a_known_harm() -> None:
    p = filed_pair()
    with pytest.raises(pair.PairError, match="no planted"):
        pair.audit(p, "none", "fine")
    p.plant = {"find": "a", "replace": "b", "expect": "c"}
    with pytest.raises(pair.PairError, match="--harm"):
        pair.audit(p, "broke it", "x")
    pair.audit(p, "fixed:E1.7", "it built a helper that should not exist")
    assert p.audit is not None and p.audit["harm"] == "fixed:E1.7"


# Filing.


def ops(f: pair.Filing, op: str) -> list[dict[str, Any]]:
    return [o for o in f.body["ops"] if o["op"] == op]


def test_a_marked_pair_with_no_copy_title_is_refused() -> None:
    with pytest.raises(pair.PairError, match="no copy title"):
        filing(filed_pair(copy_title=None), ticket())


def test_the_source_update_adds_only_the_source_arms_labels() -> None:
    source = ticket(labels=["tier:opus"])
    update = [
        o
        for o in ops(filing(filed_pair(), source, epic=epic(allocation=None)), "update")
    ]
    assert len(update) == 1
    assert update[0]["id"] == "E1.1"
    assert update[0]["add_labels"] == ["provider:claude", "effort:medium"]
    assert update[0]["set"] == {"priority": 1}


def test_both_arms_of_a_filed_pair_run_first() -> None:
    f = filing(filed_pair(), ticket(priority=4), epic=epic(allocation=None))
    assert ops(f, "update")[0]["set"] == {"priority": 1}
    assert ops(f, "create")[0]["fields"]["priority"] == 1


def test_a_hub_source_gets_the_source_arms_tier() -> None:
    source = ticket("AJNHT", parent=None, labels=["hub"], status="pending_approval")
    f = filing(filed_pair(source="AJNHT"), source)
    update = ops(f, "update")[0]
    assert "tier:opus" in update["add_labels"]
    assert update["set"]["status"] == "open"
    assert {
        "ticket": "AJNHT",
        "field": "status",
        "before": "pending_approval",
        "after": "open",
    } in f.opened


def test_a_planted_sentence_reaches_the_source_and_the_copy() -> None:
    p = filed_pair(
        plant={
            "find": "keys links by slot",
            "replace": "keys links by name",
            "expect": "x",
        }
    )
    f = filing(p, ticket(), epic=epic(allocation=None))
    assert "keys links by name" in ops(f, "update")[0]["set"]["body"]
    copy = ops(f, "create")[0]["fields"]
    assert (
        "keys links by name" in copy["body"]
        and "keys links by slot" not in copy["body"]
    )


def test_the_copy_never_names_its_source_and_starts_from_the_commit() -> None:
    source = ticket(
        body="Fix E1.1 here. Use `ignite ticket attach E1.1 <file.png>`. Then run "
        "`just bugs done E1.1 --commit <sha>`. See E1.12 too.",
    )
    f = filing(filed_pair(), source)
    copy = ops(f, "create")[0]
    text = copy["fields"]["body"] + copy["fields"]["acceptance"]
    assert scan.mention("E1.1").search(text) is None
    assert "E1.12" in text
    assert "ignite ticket attach <this ticket's ID>" in text
    assert "just bugs done E1.1" not in text
    assert f"git reset --hard {START}" in text
    assert "Never run `git fetch`" in text
    assert copy.get("parent") is None
    assert "metadata" not in copy["fields"]
    assert copy["fields"]["title"] == filed_pair().copy_title


def test_the_copy_holds_the_epics_body_and_design() -> None:
    body = ops(filing(filed_pair(), ticket()), "create")[0]["fields"]["body"]
    assert "## Context from the epic" in body
    assert "Run every command through `ignite run --`." in body


def test_the_copy_has_the_copy_arms_labels_and_the_sources_pins() -> None:
    known_epic = epic(labels=["machine:jarvis"], needs_os="macos")
    fields = ops(filing(filed_pair(), ticket(), epic=known_epic), "create")[0]["fields"]
    assert set(fields["labels"]) == {
        "trial:copy",
        "trial:pair-c0ffee00",
        "tier:sonnet",
        "provider:claude",
        "effort:high",
        "machine:jarvis",
    }
    assert fields["needs_os"] == "macos"


def test_an_unpinned_copy_runs_on_the_trials_machine() -> None:
    fields = ops(filing(filed_pair(), ticket(), home="jarvis"), "create")[0]["fields"]
    assert "machine:jarvis" in fields["labels"]


def test_an_interface_copy_must_attach_captures() -> None:
    fields = ops(filing(filed_pair(kind="interface"), ticket()), "create")[0]["fields"]
    assert "attach to this ticket" in fields["body"]
    other = ops(filing(filed_pair(), ticket()), "create")[0]["fields"]
    assert "attach to this ticket" not in other["body"]


def test_filing_opens_a_parked_epic() -> None:
    f = filing(filed_pair(), ticket(), siblings={"E1.2": 0})
    assert ops(f, "update")[0] == {"op": "update", "id": "E1", "set": {"allocation": 1}}
    assert f.opened[0] == {
        "ticket": "E1",
        "field": "allocation",
        "before": 0,
        "after": 1,
    }
    assert ops(f, "dep_add") == []
    assert f.body["if"] == [{"id": "E1.1", "version": 7}]


def test_filing_refuses_to_open_an_epic_whose_listed_tickets_are_not_parked() -> None:
    with pytest.raises(pair.PairError, match="E1.2"):
        filing(filed_pair(), ticket(), siblings={"E1.2": None})


# Grading.


def history(*changes: tuple[str, dict[str, Any]]) -> list[dict[str, Any]]:
    return [
        {"seq": i, "at": at, "actor": "x", "change": change}
        for i, (at, change) in enumerate(changes)
    ]


def test_the_source_arm_is_its_first_claim_and_first_landing() -> None:
    t = ticket(
        history=history(
            ("2026-11-24T10:00:00Z", {"assignee": [None, "cli-claude-11111111"]}),
            ("2026-11-24T10:30:00Z", {"landed": [None, {"commits": ["c1", "c2"]}]}),
            ("2026-11-24T10:30:00Z", {"status": ["in_progress", "closed"]}),
            (
                "2026-11-24T11:00:00Z",
                {"landed": [{"commits": ["c1"]}, {"commits": ["c3"]}]},
            ),
        )
    )
    claim = grade.first_claim(t)
    assert claim is not None
    assert (claim.session, claim.ended, claim.landed) == (
        "cli-claude-11111111",
        "landed",
        ["c1", "c2"],
    )
    assert grade.minutes_between(claim.claimed_at, str(claim.ended_at)) == 30.0


def test_a_released_first_claim_is_unfinished() -> None:
    t = ticket(
        history=history(
            ("2026-11-24T10:00:00Z", {"assignee": [None, "cli-claude-11111111"]}),
            ("2026-11-24T10:20:00Z", {"assignee": ["cli-claude-11111111", None]}),
            ("2026-11-24T11:00:00Z", {"assignee": [None, "cli-claude-22222222"]}),
        )
    )
    claim = grade.first_claim(t)
    assert claim is not None and claim.ended == "released" and claim.landed is None


def arm(tid: str, *traces: Trace, **fields: Any) -> scan.Arm:
    session = f"cli-claude-{tid[-1] * 8}"
    t = ticket(tid, **fields.pop("ticket", {}))
    return scan.Arm(
        ticket=t,
        sessions={session},
        traces=list(traces),
        added=fields.pop("added", []),
        tree=fields.pop("tree", None),
        elsewhere=fields.pop("elsewhere", []),
    )


def cmd(text: str) -> Trace:
    return Trace("command", text, "t:1")


def voids(source: scan.Arm, copy: scan.Arm, base: str | None = START) -> list[str]:
    return scan.void_reasons(source, copy, START, base, {"cli-claude-blockses"})


def test_a_clean_pair_is_not_void() -> None:
    copy = arm(
        "C2",
        cmd("git reset --hard " + START),
        cmd("git branch trial/C2 HEAD"),
        tree="tree-04",
    )
    assert voids(arm("E1.1"), copy) == []


@pytest.mark.parametrize(
    "trace, reason",
    [
        (cmd("ignite ticket show E1.1"), "names the source"),
        (Trace("output", "landed by E1.1", "t:2"), "names the source"),
        (cmd("git fetch origin"), "git fetch"),
        (cmd("git log origin/staging"), "under origin/"),
        (cmd("git diff trial/C9"), "trial/C9"),
        (cmd("ls /Volumes/BerylTrees/trees/tree-07/src"), "another tree"),
        (cmd("ignite ticket comment E5 'done'"), "comment on E5"),
        (cmd("ignite flaky off rust:x"), "switched a check"),
        (cmd("ignite check off rust:x"), "switched a check"),
        (cmd("just bugs done C2 --commit abc"), "just bugs done"),
    ],
)
def test_each_void_rule_of_the_copy(trace: Trace, reason: str) -> None:
    found = voids(arm("E1.1"), arm("C2", trace, tree="tree-04"))
    assert any(reason in f for f in found), found


def test_a_copy_branch_from_another_commit_is_void() -> None:
    found = voids(arm("E1.1"), arm("C2"), base="b" * 40)
    assert any("branch starts at" in f for f in found)


@pytest.mark.parametrize(
    "trace, reason",
    [
        (cmd("ignite ticket show C2"), "names the copy"),
        (cmd("git log trial/C2"), "trial/ branch"),
    ],
)
def test_each_void_rule_of_the_source(trace: Trace, reason: str) -> None:
    assert any(reason in f for f in voids(arm("E1.1", trace), arm("C2")))


def test_a_copy_touched_by_another_session_is_void() -> None:
    copy = arm(
        "C2",
        ticket={
            "comments": [{"author": "cli-claude-99999999", "text": "dup"}],
            "history": [
                {
                    "actor": "cli-claude-blockses",
                    "change": {"status": ["open", "closed"]},
                },
                {
                    "actor": "cli-codex-88888888",
                    "change": {"status": ["open", "closed"]},
                },
            ],
        },
    )
    found = voids(arm("E1.1"), copy)
    assert "cli-claude-99999999 commented on the copy" in found
    assert "cli-codex-88888888 closed the copy" in found
    assert not any("blockses" in f for f in found)


def suspects(
    source: scan.Arm, copy: scan.Arm, landed: list[str] | None = None
) -> list[str]:
    found = scan.suspect(
        source,
        copy,
        "Keep exiled-card links when a source leaves and comes back",
        landed or [],
        str(ticket()["body"]),
        set(),
    )
    return [f.what for f in found]


@pytest.mark.parametrize(
    "source_traces, copy_traces, what",
    [
        (
            [Trace("output", "C2 is open", "t")],
            [],
            "the source saw the other arm's ID C2",
        ),
        (
            [
                Trace(
                    "output",
                    "Keep exiled-card links when a source leaves and comes back",
                    "t",
                )
            ],
            [],
            "the source saw the other arm's title",
        ),
        (
            [Trace("message", "This looks like a trial.", "t")],
            [],
            "the source said 'trial'",
        ),
        (
            [],
            [Trace("message", "Sonnet should do this", "t")],
            "the copy said 'Sonnet'",
        ),
        (
            [Trace("output", "trees/tree-09 is busy", "t")],
            [],
            "the source saw the other arm's tree tree-09",
        ),
        (
            [],
            [Trace("output", "commit 0123456789abcdef", "t")],
            "the copy saw 0123456789ab, which the source landed",
        ),
    ],
)
def test_each_kind_of_evidence_marks_a_pair_suspect(
    source_traces: list[Trace], copy_traces: list[Trace], what: str
) -> None:
    found = suspects(
        arm("E1.1", *source_traces, tree="tree-03"),
        arm("C2", *copy_traces, tree="tree-09"),
        landed=["0123456789abcdef0123"],
    )
    assert what in found, found


def test_tier_labels_in_output_are_not_evidence() -> None:
    found = suspects(
        arm("E1.1", Trace("message", "The ticket is tier:opus.", "t")), arm("C2")
    )
    assert found == []


def test_comments_on_other_tickets_are_evidence() -> None:
    found = suspects(arm("E1.1", elsewhere=[("E7", "cli-claude-11111111")]), arm("C2"))
    assert found == ["the source's session cli-claude-11111111 commented on E7"]


def test_shared_distinctive_lines_mark_a_pair_suspect() -> None:
    line = "fn waiting_ability_keeps_its_incarnation_links() {"
    found = suspects(arm("E1.1", added=[line, "}"]), arm("C2", added=[line, "}"]))
    assert found == [f"both arms added {line!r}"]


def test_lines_the_ticket_dictates_are_not_suspect() -> None:
    line = "Change `engine/crates/beryl_rules/src/objects/linked.rs`."
    assert suspects(arm("E1.1", added=[line]), arm("C2", added=[line])) == []
    in_start = "let links = memo.exiled_with(slot, incarnation);"
    found = scan.suspect(
        arm("E1.1", added=[in_start]),
        arm("C2", added=[in_start]),
        "",
        [],
        "",
        {in_start},
    )
    assert found == []


# Waiting for a block's pairs.


def test_the_next_block_waits_until_both_arms_of_every_pair_have_ended() -> None:
    claimed = ("2026-11-24T10:00:00Z", {"assignee": [None, "cli-claude-11111111"]})
    closed = ("2026-11-24T11:00:00Z", {"status": ["in_progress", "closed"]})
    released = ("2026-11-24T11:00:00Z", {"assignee": ["cli-claude-11111111", None]})
    tickets = {
        "E1.1": ticket("E1.1", history=history(claimed, closed)),
        "C1": ticket("C1", history=history(claimed)),
        "E1.2": ticket("E1.2", history=history(claimed, released)),
        "C2": ticket("C2", history=history()),
    }
    pairs = [
        filed_pair(pair="p1", source="E1.1", copy="C1", state=FILED),
        filed_pair(pair="p2", source="E1.2", copy="C2", state=FILED),
        filed_pair(pair="p3", source="E1.3", state=MARKED),
    ]
    # C1 is still being worked and C2 was never claimed. E1.2's first session released it,
    # which ends its arm although the ticket stays open.
    assert pair.unfinished(pairs, tickets) == ["C1", "C2"]
    tickets["C1"] = ticket("C1", history=history(claimed, closed))
    tickets["C2"] = ticket("C2", history=history(claimed, closed))
    assert pair.unfinished(pairs, tickets) == []


def test_the_next_block_depends_on_every_arm_of_every_filed_pair() -> None:
    pairs = [
        filed_pair(pair="p1", source="E1.1", copy="C1", state=FILED),
        filed_pair(pair="p2", source="E1.2", state=MARKED),
    ]
    assert pair.wait_edges("T9.3", pairs) == [
        {"op": "dep_add", "id": "T9.3", "on": tid, "kind": "blocks", "external": True}
        for tid in ("E1.1", "C1")
    ]


# Several copy arms.

HYDROGEN = {"tier": "opus", "provider": "agy-gemini", "effort": "high"}


def two_arms() -> Spec:
    return spec(arms={**SPEC_JSON["arms"], "hydrogen": HYDROGEN})


def test_every_arm_beside_the_source_is_a_copy_arm_in_order() -> None:
    found = two_arms()
    assert found.copy_names() == ["copy", "hydrogen"]
    assert found.first_copy == "copy"
    assert found.arm("hydrogen").labels() == [
        "tier:opus",
        "provider:agy-gemini",
        "effort:high",
    ]
    with pytest.raises(SpecError, match="no arm named"):
        found.arm("neon")


def test_an_arm_name_must_be_lowercase_words() -> None:
    with pytest.raises(SpecError, match=r"arms\.Gemini"):
        spec(arms={**SPEC_JSON["arms"], "Gemini": HYDROGEN})


def test_a_row_from_before_several_arms_belongs_to_the_copy_arm() -> None:
    row = {"pair": "p1", "trial": "T1", "source": "E1.1", "order": 0}
    assert manifest._build(Pair, row).arm == "copy"


def main_and_extra() -> tuple[Pair, Pair]:
    main = filed_pair(
        state=FILED, copy="TMAIN", start_commit=START, block=1, models={"source": "o"}
    )
    main.arm_titles = {"hydrogen": "Linked exiles survive the source coming back"}
    extra = pair.extra_pair("beef0001", main, "hydrogen", "THYDR", {"copy": "g"})
    return main, extra


def test_another_arms_copy_shares_the_source_and_start_commit() -> None:
    main, extra = main_and_extra()
    assert (extra.source, extra.start_commit, extra.block) == ("E1.1", START, 1)
    assert (extra.arm, extra.main, extra.state) == ("hydrogen", main.pair, FILED)
    assert extra.copy_title == "Linked exiles survive the source coming back"
    pairs = [main, extra]
    assert manifest.extra_copies(pairs, main) == {"hydrogen": extra}
    assert manifest.find_pair(pairs, "E1.1") is main
    assert manifest.find_pair(pairs, "THYDR") is extra


def test_pair_file_finds_the_filed_sources_an_arm_has_no_copy_of() -> None:
    main, extra = main_and_extra()
    listed = filed_pair(pair="p2", source="E1.2", state=LISTED)
    assert pair.missing_extras(two_arms(), [main, listed]) == [(main, "hydrogen")]
    assert pair.missing_extras(two_arms(), [main, extra, listed]) == []
    assert pair.missing_extras(spec(), [main, listed]) == []


def test_another_arms_copy_does_not_count_toward_the_trials_limit() -> None:
    main, extra = main_and_extra()
    listed = filed_pair(pair="p2", source="E1.2", state=LISTED, order=1)
    marked, _ = pair.mark_block([main, extra, listed], 2, 6, 2)
    assert marked == [listed]


def test_another_arms_copy_clones_the_main_copys_text_with_its_own_routing() -> None:
    main_copy = ticket(
        "TMAIN",
        title="Keep exiled-card links when a source leaves and comes back",
        body="The copy text.",
        acceptance="It works.",
        labels=[
            "trial:copy",
            "trial:pair-c0ffee00",
            "tier:sonnet",
            "provider:claude",
            "effort:high",
            "machine:jarvis",
        ],
        needs_os="macos",
    )
    arm = two_arms().arm("hydrogen")
    body = pair.build_extra("beef0001", arm, "Another wording", main_copy)
    (op,) = body["ops"]
    fields = op["fields"]
    assert (fields["title"], fields["body"], fields["acceptance"]) == (
        "Another wording",
        "The copy text.",
        "It works.",
    )
    assert fields["labels"] == [
        "trial:copy",
        "trial:pair-beef0001",
        "tier:opus",
        "provider:agy-gemini",
        "effort:high",
        "machine:jarvis",
    ]
    assert (fields["needs_os"], fields["priority"]) == ("macos", 1)


def test_a_copy_title_must_differ_from_the_other_copies() -> None:
    with pytest.raises(pair.PairError, match="other copies"):
        pair.check_title("Some other words", ticket(), ["some other words"])


def test_a_lighter_helper_model_or_the_effort_suffix_does_not_void_an_arm() -> None:
    requests = {"gemini-4-hydrogen-high": 400, "gemini-3.8-flash-low": 3}
    assert grade.other_models(requests, "gemini-4-hydrogen") == []
    requests["gemini-4-neon-high"] = 100
    assert grade.other_models(requests, "gemini-4-hydrogen") == ["gemini-4-neon-high"]


def test_an_agy_transcript_gives_messages_commands_and_output(tmp_path: Any) -> None:
    logs = tmp_path / "conv-1" / ".system_generated" / "logs"
    logs.mkdir(parents=True)
    path = logs / "transcript.jsonl"
    records = [
        {
            "type": "PLANNER_RESPONSE",
            "created_at": "2026-11-24T10:00:00Z",
            "content": "This is a trial copy.",
            "tool_calls": [
                {"name": "run_command", "args": {"CommandLine": "git fetch"}}
            ],
        },
        {"type": "GENERIC", "created_at": "2026-11-24T10:00:01Z", "content": "done"},
        {"type": "GENERIC", "created_at": "2026-11-25T00:00:00Z", "content": "late"},
    ]
    path.write_text("\n".join(json.dumps(r) for r in records), encoding="utf-8")
    found = grade.agy_traces(path, "2026-11-24T00:00:00Z", "2026-11-24T23:59:59Z")
    assert [(t.kind, t.where) for t in found] == [
        ("message", "conv-1:1"),
        ("command", "conv-1:1"),
        ("output", "conv-1:2"),
    ]
    assert "git fetch" in found[1].text


def test_seeing_another_arms_copy_marks_a_pair_suspect() -> None:
    source = arm("E1.1")
    copy = arm("TC1", cmd("ignite ticket show THYDR"))
    found = scan.suspect(source, copy, "", [], "", set(), [("THYDR", "Other words")])
    assert any("another arm's copy THYDR" in f.what for f in found)
