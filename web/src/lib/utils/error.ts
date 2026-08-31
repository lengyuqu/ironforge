// Error utilities — normalise unknown catch values into user-friendly strings.
// Every `catch (e)` clause should call `toErrorMessage(e)` instead of `e.message`
// directly; TS strict mode types catch bindings as `unknown`, so `.message` is
// not available without a type guard.

/**
 * Extract a human-readable message from an unknown catch value.
 * Handles standard Error (with optional code/request_id extras), Error-like
 * objects from the backend, and primitives (strings, numbers) that some code
 * paths might throw directly.
 */
export function toErrorMessage(e: unknown, fallback = 'Unknown error'): string {
  if (e instanceof Error) {
    // Backend errors sometimes attach code/request_id as extra properties.
    const err = e as Error & { code?: string | number; request_id?: string; requestId?: string };
    const parts: string[] = [e.message];
    if (err.code) parts.push(`(${err.code})`);
    const rid = err.request_id ?? err.requestId;
    if (rid) parts.push(`[request ${rid}]`);
    return parts.join(' ');
  }
  if (typeof e === 'string') return e;
  if (typeof e === 'number' || typeof e === 'boolean') return String(e);
  // Duck-typed error-like object from JSON fetch rejection.
  if (e && typeof e === 'object') {
    const maybe = e as { message?: string; error?: string };
    if (maybe.message) return maybe.message;
    if (maybe.error) return String(maybe.error);
  }
  return fallback;
}
