// The prompt bar on the stage's boards (plan K26): what the open prompt asks and its buttons,
// each as the session sent it. Its words, which button it puts forward and whether one can be
// pressed are the session's (prompt.ts); a press goes back to it. What is laid out here is
// only where each stands: the button put forward at the foot of the desktop's dock, where the
// turn button is, with what it says under it; the button that passes on to a later stop as the
// round one beside it; and the others stacked over, in the order they came. A cost paid with
// permanents says after what it asks how much is still to pay, and its Auto Pay shows what
// it would tap while the pointer is over it or a touch is held on it (UI Conventions,
// "Paying with permanents").
import {
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type ButtonHTMLAttributes,
} from "react";
import {
  Button,
  Dock,
  DockButtons,
  FastForwardIcon,
  Tip,
  TurnButton,
} from "../components/Controls";
import { RulesText } from "../components/Mana";
import type {
  ButtonId,
  PromptButton,
  PromptView,
  Side,
} from "../generated/protocol";
import type { Asked, Prompts } from "./prompt.ts";

/** What the page's stage asks, as the screen draws it. */
export const usePrompt = (prompts: Prompts) =>
  useSyncExternalStore(prompts.subscribe, prompts.asked);

/** The answers a board gives besides the prompt's own buttons. */
interface Answers {
  /** A view is on its way, or the game plays on without the player. */
  waiting: boolean;
  press(button: ButtonId): void;
  /** Takes back the cast the open decision is part of, where it may be. */
  cancel?: () => void;
  /** Shows or hides what the open payment's Auto Pay would tap (`usePaying`). */
  suggest?: (on: boolean) => void;
}

/** What a board shows of a cost paid with permanents: `pay`, for its `data-pay`, is `quiet`
 * while the open prompt is such a payment and `suggesting` while it shows what Auto Pay would
 * tap (stage.css), and `suggest` is the dock's to call. */
export function usePaying({ prompt }: Asked) {
  const [suggesting, suggest] = useState(false);
  const paying = prompt?.state?.shape === "pay";
  // A button that goes while the pointer is on it never says the pointer left.
  useEffect(() => {
    if (!paying) suggest(false);
  }, [paying]);
  const pay = !paying ? undefined : suggesting ? "suggesting" : "quiet";
  return { pay, suggest } as const;
}

/** How long a touch is held on Auto Pay before it shows what it would tap, in ms. */
const HELD = 450;

/** What each button of `prompt` does with the pointer: a press of it, and for a payment's Auto
 * Pay the showing of what it would tap, while the pointer is over it or a touch is held on
 * it. A touch held only shows: letting go of it presses nothing. */
function usePresses(
  prompt: PromptView | undefined,
  { press, suggest }: Pick<Answers, "press" | "suggest">,
) {
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const held = useRef(false);
  useEffect(() => () => clearTimeout(timer.current), []);
  return ({ id }: PromptButton): ButtonHTMLAttributes<HTMLButtonElement> => {
    const shows =
      suggest && id.kind === "auto" && prompt?.state?.shape === "pay";
    if (!shows) return { onClick: () => press(id) };
    const end = () => {
      clearTimeout(timer.current);
      suggest(false);
    };
    return {
      onPointerEnter: (e) => {
        if (e.pointerType === "mouse") suggest(true);
      },
      onPointerLeave: end,
      onPointerDown: (e) => {
        held.current = false;
        if (e.pointerType === "mouse") return;
        timer.current = setTimeout(() => {
          held.current = true;
          suggest(true);
        }, HELD);
      },
      onPointerUp: (e) => {
        if (e.pointerType !== "mouse") end();
      },
      onPointerCancel: end,
      onContextMenu: (e) => e.preventDefault(),
      onClick: (e) => {
        // A key's press has no touch behind it.
        if (!held.current || e.detail === 0) {
          suggest(false);
          press(id);
        }
        held.current = false;
      },
    };
  };
}

/** What a prompt asks, and for a cost paid with permanents how much of it is still to pay, as
 * the session wrote each. */
function Asks({ prompt: { title, state } }: { prompt: PromptView }) {
  return (
    <>
      <RulesText text={title} />
      {state?.shape === "pay" && (
        <span className="prompt-left">
          {" ("}
          <RulesText text={state.left} />
          {")"}
        </span>
      )}
    </>
  );
}

/** A prompt's buttons by where they stand: the one put forward, the one that passes on to a
 * later stop, and the others in the order the session laid them out. */
function standing(buttons: readonly PromptButton[]) {
  const forward = buttons.find((button) => button.primary);
  const passOn = buttons.find((button) => button.id.kind === "passOn");
  const others = buttons.filter(
    (button) => button !== forward && button !== passOn,
  );
  return { forward, passOn, others };
}

const keyOf = ({ id }: PromptButton) =>
  id.kind === "option" ? `option-${id.index}` : id.kind;

/** The keys that answer the open prompt, as the old board has them: Space or Enter presses
 * the button put forward, Shift with either the one that passes on, and Escape takes a cast
 * back. A control that has the focus keeps its own key, and `idle` is false while something
 * else on the screen takes the keys. */
export function usePromptKeys(
  { prompt, decision, sending }: Asked,
  { press, cancel }: Pick<Answers, "press" | "cancel">,
  idle: boolean,
) {
  const onKey = useRef<(e: KeyboardEvent) => void>(undefined);
  onKey.current = (e) => {
    if (!prompt || sending || !idle || e.repeat) return;
    if (e.key === "Escape" && decision?.cancellable && cancel) {
      e.preventDefault();
      cancel();
      return;
    }
    if (e.key !== " " && e.key !== "Enter") return;
    const control =
      e.target instanceof Element &&
      e.target.closest("button, input, select, textarea, a");
    if (control) return;
    const { forward, passOn } = standing(prompt.buttons);
    const button = e.shiftKey ? passOn : forward;
    if (!button?.enabled) return;
    e.preventDefault();
    press(button.id);
  };
  useEffect(() => {
    const heard = (e: KeyboardEvent) => onKey.current?.(e);
    window.addEventListener("keydown", heard);
    return () => window.removeEventListener("keydown", heard);
  }, []);
}

/** A player's life total as the open prompt marks it: its face takes the click, which goes to
 * the session as one on that player. Nothing where the prompt marks no such player. */
export function PlayerTarget({
  prompts,
  asked: { prompt },
  side,
}: {
  prompts: Prompts;
  asked: Asked;
  side: Side;
}) {
  const marked = prompt?.marks.some(
    ({ on }) => on.kind === "player" && on.player === side,
  );
  if (!marked) return null;
  return (
    <span
      className="life-hit life-target"
      role="button"
      aria-label={side === "you" ? "Target yourself" : "Target opponent"}
      onClick={() => prompts.on({ kind: "player", player: side })}
    />
  );
}

/** The round button that passes on to a later stop, which its label names. */
function PassOn({
  button,
  disabled,
  press,
  tip,
}: {
  button: PromptButton;
  disabled: boolean;
  press(button: ButtonId): void;
  tip?: boolean;
}) {
  return (
    <button
      type="button"
      className={tip ? "pass-on-button has-tip" : "pass-on-button"}
      aria-label={button.label}
      disabled={disabled}
      onPointerDown={(e) => e.stopPropagation()}
      onClick={() => press(button.id)}
    >
      <span className="pass-on-face button-primary">
        <FastForwardIcon />
      </span>
      {tip && (
        <Tip side="above">
          {button.label}
          <kbd>⇧ Space</kbd>
        </Tip>
      )}
    </button>
  );
}

/** The desktop's dock for the open prompt. While the game plays on, the turn button stands
 * where it will be with nothing to press, as the old board's does. */
export function PromptDock({
  asked: { prompt, decision, sending },
  waiting,
  press,
  cancel,
  suggest,
}: { asked: Asked } & Answers) {
  const presses = usePresses(prompt, { press, suggest });
  if (!prompt) {
    return waiting ? (
      <Dock className="stage-prompt">
        <div className="turn-action">
          <TurnButton label="Pass" disabled />
        </div>
      </Dock>
    ) : null;
  }
  const { forward, passOn, others } = standing(prompt.buttons);
  const off = (button: PromptButton) => sending || !button.enabled;
  const takesBack = decision?.cancellable && cancel;
  return (
    <Dock
      className="stage-prompt"
      title={prompt.title ? <Asks prompt={prompt} /> : undefined}
    >
      {(others.length > 0 || takesBack) && (
        <DockButtons>
          {others.map((button) => (
            <Button
              key={keyOf(button)}
              variant="secondary"
              disabled={off(button)}
              {...presses(button)}
            >
              <RulesText text={button.label} />
            </Button>
          ))}
          {takesBack && (
            <Button variant="secondary" disabled={sending} onClick={cancel}>
              {prompt.words.cancel}
            </Button>
          )}
        </DockButtons>
      )}
      {forward && (
        <div className="turn-action">
          <TurnButton
            label={forward.label}
            disabled={off(forward)}
            {...presses(forward)}
          />
          {passOn && (
            <PassOn button={passOn} disabled={off(passOn)} press={press} tip />
          )}
          {forward.caption && (
            <span className="turn-caption">{forward.caption}</span>
          )}
        </div>
      )}
    </Dock>
  );
}

/** The phone's dock for the open prompt, in the player's strip: the button put forward and
 * the one that passes on. */
export function PhonePromptDock({
  asked: { prompt, sending },
  waiting,
  press,
  suggest,
}: { asked: Asked } & Answers) {
  const presses = usePresses(prompt, { press, suggest });
  if (!prompt) {
    return waiting ? (
      <div className="phone-dock">
        <TurnButton label="Pass" disabled />
      </div>
    ) : null;
  }
  const { forward, passOn } = standing(prompt.buttons);
  if (!forward && !passOn) return null;
  return (
    <div className="phone-dock">
      {forward && (
        <TurnButton
          label={forward.label}
          disabled={sending || !forward.enabled}
          {...presses(forward)}
        />
      )}
      {passOn && (
        <PassOn
          button={passOn}
          disabled={sending || !passOn.enabled}
          press={press}
        />
      )}
    </div>
  );
}

/** What of the open prompt stands over the phone's player strip, whose top is `top`: what it
 * asks, and its other buttons stacked over the dock. */
export function PhonePromptOver({
  asked: { prompt, decision, sending },
  press,
  cancel,
  suggest,
  top,
}: { asked: Asked; top: number } & Answers) {
  const presses = usePresses(prompt, { press, suggest });
  if (!prompt) return null;
  const { others } = standing(prompt.buttons);
  const takesBack = decision?.cancellable && cancel;
  return (
    <>
      {prompt.title && (
        <div className="phone-pill" role="status" style={{ top: top - 30 }}>
          <Asks prompt={prompt} />
        </div>
      )}
      {(others.length > 0 || takesBack) && (
        <div
          className="phone-prompt-more"
          style={{ bottom: `calc(100% - ${top - 6}px)` }}
        >
          {others.map((button) => (
            <Button
              key={keyOf(button)}
              variant="secondary"
              disabled={sending || !button.enabled}
              {...presses(button)}
            >
              <RulesText text={button.label} />
            </Button>
          ))}
          {takesBack && (
            <Button variant="secondary" disabled={sending} onClick={cancel}>
              {prompt.words.cancel}
            </Button>
          )}
        </div>
      )}
    </>
  );
}
