let tableOpen = false;

/** The table's keyboard listener sets this for the lifetime of the table screen. */
export function setBugReportTableOpen(open: boolean) {
  tableOpen = open;
}

type KeyEvent = Pick<
  KeyboardEvent,
  "code" | "key" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey"
>;

export function isBugReportShortcut(event: KeyEvent): boolean {
  return (
    (event.metaKey || event.ctrlKey) &&
    !event.altKey &&
    event.shiftKey === tableOpen &&
    (tableOpen ? event.code === "KeyB" : event.key.toLowerCase() === "b")
  );
}
