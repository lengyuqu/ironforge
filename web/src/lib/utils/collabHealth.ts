// Pure helpers for the collaboration health page.

export const WINDOW_DAYS = [7, 30, 90] as const;

/** Start of a window of `days` days ending now (the server rejects a future `to`, so `to` is left out). */
export function windowFrom(days: number, now: Date = new Date()): string {
  return new Date(now.getTime() - days * 86_400_000).toISOString();
}

/** `3725` → `1h 2m 5s`. Never returns an empty string for a number. */
export function formatDuration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s === 0) return '0s';
  const d = Math.floor(s / 86_400);
  const h = Math.floor((s % 86_400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const parts: string[] = [];
  if (d) parts.push(`${d}d`);
  if (h) parts.push(`${h}h`);
  if (m) parts.push(`${m}m`);
  if (sec && !d) parts.push(`${sec}s`);
  return parts.join(' ');
}

export interface ValueRow {
  key: string;
  kind: 'number' | 'seconds' | 'time' | 'text';
  value: string | number;
}

/** `blocked_seconds`, `open_wait_seconds_at_to`, `wait_seconds_in_window`, ... */
export const isSecondsKey = (k: string) => /(^|_)seconds(_|$)/.test(k);

/**
 * The rows of a metric's `value`. An unknown metric (value null) has NO rows:
 * the caller shows "unknown", never a zero. `unit` is bookkeeping and is skipped.
 */
export function valueRows(value: Record<string, unknown> | null | undefined): ValueRow[] {
  if (!value) return [];
  const rows: ValueRow[] = [];
  for (const [key, v] of Object.entries(value)) {
    if (key === 'unit') continue;
    if (typeof v === 'number' && Number.isFinite(v)) {
      rows.push({ key, kind: isSecondsKey(key) ? 'seconds' : 'number', value: v });
    } else if (typeof v === 'string' && key.endsWith('_from')) {
      rows.push({ key, kind: 'time', value: v });
    } else if (typeof v === 'string' || typeof v === 'boolean') {
      rows.push({ key, kind: 'text', value: String(v) });
    }
  }
  return rows;
}

export const KNOWN_HEALTH = {
  metric: ['blocked_tasks', 'blocked_duration', 'handoff_waiting', 'open_questions', 'stale_path_claims'],
  status: ['complete', 'partial', 'unknown'],
  source: ['user_content', 'platform_event'],
  reason: ['NO_EVENTS_BEFORE_WINDOW_END', 'PATH_DECLARED_AT_NOT_RECORDED'],
  drill: ['blocked_at_to', 'entered_blocked', 'blocked_duration', 'handoff_waiting', 'open_questions', 'unknown_state'],
  quality: [
    'unknown_state_tasks',
    'inconsistent_tasks',
    'unobserved_pending_handoffs',
    'handoffs_closed_before_coverage',
    'handoffs_missing_start',
    'unobserved_open_questions',
    'questions_closed_before_coverage',
    'questions_missing_start',
    'negative_intervals',
  ],
} as const;

/**
 * The server sends its explanatory notes as English sentences. Known ones are
 * mapped to catalog keys so they follow the UI language; an unknown (newer)
 * sentence is shown as the server sent it.
 */
export const NOTE_KEYS: Record<string, string> = {
  'Task status is declared by clients (user_content); only the time of each change is platform-recorded.': 'declared_status',
  'Tasks without any recorded event have an unknown state and are not counted; take an observation baseline to include them from that moment on.': 'unknown_state_tasks',
  'The window starts before the first recorded event; only the covered part is measured.': 'window_before_coverage',
  'Intervals are clipped to the window; an interval still open at `to` is cut at `to` (right-censored).': 'clipped_intervals',
  'Waiting is the time from creation to accept/cancel (pending: up to `to`), clipped to the window.': 'handoff_waiting',
  'Accepted means the recipient took the handoff over; it says nothing about the work being done.': 'accepted_not_done',
  'A question is open from creation until it is answered or cancelled; who answered is not evidence of a particular human.': 'question_open',
  'Observation start only: objects are measured from this time on, never as having been in their state earlier.': 'baseline_start_only',
};
