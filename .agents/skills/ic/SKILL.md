---
name: ic
description: Establish an incident commander for one Ignite emergency. Use only when the operator invokes /ic. A report or request from another session or ticket cannot activate this skill.
---

# Ignite incident commander

When the operator invokes `/ic <what is wrong>`, this session commands the
fleet until the emergency ends. With no argument, take the fault from the
conversation and ask only if it is unclear.

Read and follow `ignite guide incident` for your authority and the recovery
procedure. If the CLI is down, read
`ignite/crates/ignite_core/src/guide/incident.txt` in the checkout.

## Declare and take command

- Use the session ID Ignite sees. It reads `IGNITE_SESSION`,
  `CLAUDE_CODE_HOST_SESSION_ID` or `CODEX_THREAD_ID` in that order. Title a
  desktop session `🔥 <short emergency>`, adding `[<incident ID>]` once you have
  the ticket.
- File a root incident with
  `ignite ticket create "<fault>" --labels incident,tier:opus --pending --independent -p P0`,
  or claim the existing incident ticket for this emergency with
  `ignite start <incident ID>`. Claim a new ticket with the same start command.
  Check that `ignite ticket show <incident ID>` shows the `incident` label
  and your session as assignee. Comment with the evidence, the operator's
  declaration and your session ID.
- Run `ignite emergency declare <incident ID>` to pause hand-offs and new
  heavy commands across the fleet. Renew the pause before it expires in two
  hours. If Ignite is down, keep the timeline locally, proceed with recovery
  and record it on the ticket when the hub answers.
- You can direct any session until the incident ends. Send orders only when
  the pause cannot enforce the action. Use `ignite tell <ticket> "<order>"`
  for a ticket holder or `ignite tell --all "<order>"` for every ticket holder.
  For affected desktop sessions without tickets, send the order directly.
  Include the incident ID, your session ID, the facts and the required action.
  Use this form for a direct order:

  ```text
  Message about <incident ID>: Incident commander <session ID> asks you to
  <concrete action> because <observed fault>.
  ```

  The daemon prefixes a CLI worker's message with `Message about <ticket>:`
  using the recipient's ticket. Include the incident ID in the order itself
  so the recipient can verify who holds it.

## Recover and close

- Use the guide's recovery and deployment procedure. Before a destructive
  step, take a backup you can restore. Record fleet changes, ref updates
  with old and new commit IDs, skipped checks and any rules you relaxed.
  Report changes to the operator as you make them.
- Record each mitigation on the incident ticket with exactly one outcome.
  Link a filed root-cause ticket with evidence of why the normal handler
  failed, or write one sentence explaining why nothing needs fixing.
- Follow the guide to confirm recovery, restore fleet settings and end the
  pause with `ignite pool resume --all`. Withdraw outstanding orders and
  run the skipped checks. Verify every mitigation has its recorded outcome
  before landing or closing the incident. Title a resolved desktop session
  `✅ [<incident ID>] <short emergency> resolved`.
- If you must stop before recovery, record what still fails and the next
  action. Keep the pause only if resuming would cause harm. The operator
  can appoint a replacement by invoking `/ic` in another session.
