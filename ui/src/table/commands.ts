import registry from "../../../data/table-commands.json" with { type: "json" };

export type AppliesTo =
  | "permanent"
  | "their_permanent"
  | "hand_card"
  | "stack_entry"
  | "pile_card"
  | "library"
  | "graveyard"
  | "exile"
  | "sideboard"
  | "hand"
  | "life"
  | "player"
  | "table"
  | "lobby"
  | "viewer_card";

export type Scope = "table" | "lobby";
export type Assist =
  "turn_upkeep" | "combat" | "state_nudges" | "library_flows" | "ability_menus";

export type CommandKey = { mac: string; other: string };
export type MenuPlacement = { group: string; order: number; submenu?: string };

type Template =
  | string
  | number
  | boolean
  | null
  | readonly Template[]
  | { readonly [field: string]: Template };

export type Command = {
  id: string;
  label: string;
  applies_to: AppliesTo;
  scope: Scope;
  asks: "none" | "number" | "text" | "stats" | "target" | "token";
  key?: CommandKey;
  key_aliases?: readonly CommandKey[];
  menu?: MenuPlacement;
  also_applies_to?: Partial<Record<AppliesTo, boolean | MenuPlacement>>;
  group: string;
  assist?: Assist;
  when?: string;
} & (
  | { action: Readonly<Record<string, Template>>; client?: never }
  | { client: string; action?: never }
);

// TypeScript widens the JSON's string literals. A Rust test checks each
// action template against the wire format after substituting sample values.
export const commands = registry as readonly Command[];

export type CommandContext = {
  scope: Scope;
  assists?: ReadonlySet<Assist>;
  // The caller derives these conditions from its view of the table.
  conditions?: ReadonlySet<string>;
};

/** Return matching commands with the menu placement for this target. */
export function commandsFor(
  appliesTo: AppliesTo,
  context: CommandContext,
): Command[] {
  return commands.flatMap((command) => {
    if (
      command.scope !== context.scope ||
      (command.assist && !context.assists?.has(command.assist)) ||
      (command.when && !context.conditions?.has(command.when))
    ) {
      return [];
    }
    if (command.applies_to === appliesTo) return [command];
    const placement = command.also_applies_to?.[appliesTo];
    if (placement === undefined) return [];
    return [
      {
        ...command,
        menu: placement === true ? command.menu : placement || undefined,
      },
    ];
  });
}
