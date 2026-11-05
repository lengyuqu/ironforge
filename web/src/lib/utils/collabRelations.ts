// Pure helpers for the "relations" part of the collaboration UI (dependencies,
// reassign, takeover history, budgets). No DOM access, so they are unit-testable.
import { ApiError } from '$lib/api/_base.svelte';
import { toErrorMessage } from '$lib/utils/error';

export type ErrorContext = 'deps' | 'reassign' | 'budget';

/** Coded errors each write documents; anything else shows the server's message alone. */
const EXPLAINED: Record<ErrorContext, readonly string[]> = {
  deps: ['VERSION_CONFLICT', 'DEPENDENCY_CYCLE', 'GRAPH_LIMIT_EXCEEDED', 'DEPENDENCY_EXISTS', 'IDEMPOTENCY_KEY_REUSED'],
  reassign: ['VERSION_CONFLICT', 'IDEMPOTENCY_KEY_REUSED'],
  budget: ['VERSION_CONFLICT', 'BUDGET_LIMIT_REACHED', 'USAGE_LIMIT_REACHED', 'IDEMPOTENCY_KEY_REUSED'],
};

/**
 * Translation key explaining a failed write in plain words, or null when the
 * server's own message is all there is to say. Coded errors win; otherwise the
 * HTTP status (403 / 404) is explained.
 */
export function explainKey(ctx: ErrorContext, e: unknown): string | null {
  if (!(e instanceof ApiError)) return null;
  if (e.code && EXPLAINED[ctx].includes(e.code)) return `collab_relations.err_${ctx}.${e.code}`;
  if (e.status === 403) return 'collab_relations.err_forbidden';
  if (e.status === 404) return 'collab_relations.err_not_found';
  return null;
}

/** After these the data the form was built from is out of date and must be re-read. */
export function mustReload(e: unknown): boolean {
  return e instanceof ApiError && e.code === 'VERSION_CONFLICT';
}

/** An `ApiError` carrying an HTTP status of 403 or 401 (not allowed to read or write). */
export function isDenied(e: unknown): boolean {
  return e instanceof ApiError && (e.status === 403 || e.status === 401);
}

/** UTF-8 byte length, the unit the server limits a reason in. */
export function byteLength(text: string): number {
  return new TextEncoder().encode(text).length;
}

/** The server's amount grammar: no sign, exponent or separators; at most 18 integer and 6 fractional digits. */
const AMOUNT_RE = /^(0|[1-9]\d{0,17})(\.\d{1,6})?$/;
export function isValidAmount(text: string): boolean {
  return AMOUNT_RE.test(text.trim());
}

/** Unit and source are compared exactly by the server: 1–`max` characters of `[A-Za-z0-9._:/@+-]`. */
export function isValidToken(text: string, max: number): boolean {
  return text.length >= 1 && text.length <= max && /^[A-Za-z0-9._:/@+-]+$/.test(text);
}

/** `<input type="datetime-local">` text (local time) → UTC ISO text, or null when it does not parse. */
export function localToIso(value: string): string | null {
  if (!value) return null;
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? null : d.toISOString();
}

/**
 * What to show for a failed write: the plain-words explanation when there is
 * one, followed by the server's own message (and code) so nothing is hidden.
 */
export function errorText(ctx: ErrorContext, e: unknown, t: (key: string) => string): string {
  const server = toErrorMessage(e, t('common.unknown_error'));
  const key = explainKey(ctx, e);
  return key ? `${t(key)} — ${server}` : server;
}
