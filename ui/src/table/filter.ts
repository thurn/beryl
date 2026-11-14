/** Prefer labels with a word prefix match, preserving input order within each rank. */
export function match<Command extends { label: string }>(
  query: string,
  commands: readonly Command[],
): Command[] {
  const words = query.toLowerCase().trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [...commands];

  const prefixes: Command[] = [];
  const substrings: Command[] = [];
  for (const command of commands) {
    const label = command.label.toLowerCase();
    if (!words.every((word) => label.includes(word))) continue;
    const labelWords = label.split(/[^\p{L}\p{N}]+/u);
    const prefix = words.some((word) =>
      labelWords.some((labelWord) => labelWord.startsWith(word)),
    );
    (prefix ? prefixes : substrings).push(command);
  }
  return [...prefixes, ...substrings];
}
