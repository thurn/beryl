import type { TableAction } from "../generated/protocol.ts";
import type { Command, Template } from "./commands.ts";

type Value = Template | TableAction;
export type ActionValues = Partial<Record<string, Value>>;

function substitute(template: Template, values: ActionValues): Value {
  if (typeof template === "string" && template.startsWith("$")) {
    const name = template.slice(1);
    const value = Object.hasOwn(values, name) ? values[name] : undefined;
    if (value === undefined)
      throw new Error(`Missing table action value ${template}`);
    return value;
  }
  if (Array.isArray(template))
    return template.map((item: Template) =>
      substitute(item, values),
    ) as Template[];
  if (template && typeof template === "object")
    return Object.fromEntries(
      Object.entries(template).map(([key, value]) => [
        key,
        substitute(value, values),
      ]),
    );
  return template;
}

/** Registry templates are checked against TableAction by the Rust registry tests. */
export function fill(
  template: NonNullable<Command["action"]>,
  values: ActionValues,
): TableAction {
  return substitute(template, values) as TableAction;
}

export function uses(template: Template | undefined, name: string): boolean {
  if (template === `$${name}`) return true;
  return (
    !!template &&
    typeof template === "object" &&
    Object.values(template).some((value: Template) => uses(value, name))
  );
}
