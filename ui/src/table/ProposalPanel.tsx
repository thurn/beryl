import { useId, useState } from "react";
import { Button } from "../components/Controls";
import type { ProposalView } from "../generated/protocol";
import { shakeRefusal } from "../stage/Refusal";
import type { AssistAnswer, TableClient } from "./client";
import "./assists.css";

export function ProposalPanel({
  proposals,
  client,
}: {
  proposals: readonly ProposalView[];
  client: TableClient;
}) {
  return (
    <div className="table-proposals" data-table-proposals="">
      {proposals.map((proposal) => (
        <Proposal key={proposal.id} proposal={proposal} client={client} />
      ))}
    </div>
  );
}

function Proposal({
  proposal,
  client,
}: {
  proposal: ProposalView;
  client: TableClient;
}) {
  const heading = useId();
  const [sending, setSending] = useState(false);
  const { id, revision, title, lines } = proposal;

  function send(answer: AssistAnswer, source: HTMLElement) {
    if (sending) return;
    setSending(true);
    void client
      .answerAssist(answer, source)
      .catch(() => shakeRefusal(source))
      .finally(() => setSending(false));
  }

  return (
    <section className="table-proposal" aria-labelledby={heading}>
      <h2 id={heading}>{title}</h2>
      <ul className="table-proposal-lines">
        {lines.map((line) => (
          <li key={line.id}>
            <label>
              <input
                type="checkbox"
                checked={line.on}
                onChange={(event) =>
                  send(
                    {
                      kind: "proposalLine",
                      id,
                      line: line.id,
                      on: event.currentTarget.checked,
                    },
                    event.currentTarget,
                  )
                }
              />
              <span>{line.text}</span>
              <span className="table-proposal-whose">
                {line.whose === "you" ? "yours" : "theirs"}
              </span>
            </label>
          </li>
        ))}
      </ul>
      <Button
        size="small"
        disabled={sending || !lines.some((line) => line.on)}
        onClick={(event) =>
          send({ kind: "applyProposal", id, revision }, event.currentTarget)
        }
      >
        Apply
      </Button>
    </section>
  );
}
