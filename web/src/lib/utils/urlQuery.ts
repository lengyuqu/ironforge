/**
 * Parse a 1-based positive integer page query parameter from `url`.
 * Falls back to `1` for missing, non-integer, zero, negative, or unsafe values.
 */
export function readPageParam(url: URL, key = 'page'): number {
  const raw = url.searchParams.get(key);
  if (!raw || !/^\d+$/.test(raw)) return 1;
  const parsed = Number(raw);
  return Number.isSafeInteger(parsed) && parsed >= 1 ? parsed : 1;
}

/**
 * Read a string query parameter constrained to `allowed` values, returning
 * `fallback` when missing or unrecognized.
 */
export function readEnumParam<T extends string>(
  url: URL,
  key: string,
  fallback: T,
  allowed: readonly T[],
): T {
  const raw = url.searchParams.get(key);
  return raw !== null && (allowed as readonly string[]).includes(raw) ? (raw as T) : fallback;
}

/**
 * Build a pathname + query string by cloning `url.searchParams` and applying
 * `updates`. Values that are `null`, `undefined`, `''`, or `false` delete the key.
 */
export function buildQueryHref(
  url: URL,
  updates: Record<string, string | number | boolean | null | undefined>,
): string {
  const params = new URLSearchParams(url.searchParams);
  for (const [key, value] of Object.entries(updates)) {
    if (value === null || value === undefined || value === '' || value === false) {
      params.delete(key);
    } else {
      params.set(key, String(value));
    }
  }
  const qs = params.toString();
  return qs ? `${url.pathname}?${qs}` : url.pathname;
}
