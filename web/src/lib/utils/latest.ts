/**
 * Sequence guard for async loads so stale responses from superseded requests
 * do not overwrite newer state.
 */
export function createLatest() {
  let seq = 0;
  return {
    /** Call at the start of a load; the returned fn reports whether this load is still the newest. */
    begin(): () => boolean {
      const id = ++seq;
      return () => id === seq;
    },
    invalidate() {
      seq++;
    },
  };
}
