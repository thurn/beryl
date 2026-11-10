// A temporary frame breaks up along a stippled edge. Only departing objects use the mask.
export function dissolve(reduce: boolean) {
  return reduce
    ? { opacity: 0, transition: { duration: 0.09 } }
    : {
        maskPosition: ["50% 100%", "50% -10%"],
        transition: { duration: 0.32, ease: "easeInOut" as const },
      };
}
