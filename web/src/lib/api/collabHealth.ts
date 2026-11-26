// Collaboration health metrics — human-facing REST client (repository admins only).
//
// Contract facts the UI relies on (see
// docs/design/agent-collaboration.md §7):
//   * Metrics come from a platform event log. Nothing before `coverage_start`
//     is measured: such values are `partial` (a lower bound) or `unknown`
//     (`value` is null) and must never be shown as 0.
//   * Task status itself is a client declaration (`source: user_content`);
//     only the time of each change is platform-recorded.
//   * There is no per-session / per-account / per-agent breakdown.
import { request, qs } from './_base.svelte';

const base = (owner: string, repo: string) =>
  `/repos/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/collaboration/health`;

export interface HealthDrillLink {
  metric: string;
  href: string;
}

export interface HealthMetric {
  id: string;
  definition_version: number;
  /** Who vouches for the values: `user_content` (a declaration) or `platform_event`. */
  source: string;
  timing_source: string;
  /** `complete` | `partial` (lower bound) | `unknown` (value is null). */
  status: string;
  value: Record<string, unknown> | null;
  reason: string | null;
  notes: string[];
  drill_down: HealthDrillLink[];
}

export interface HealthRange {
  from: string;
  to: string;
  timezone: string;
  interval: string;
}

export interface HealthDataQuality {
  unknown_state_tasks: number;
  inconsistent_tasks: number;
  unobserved_pending_handoffs: number;
  handoffs_closed_before_coverage: number;
  handoffs_missing_start: number;
  unobserved_open_questions: number;
  questions_closed_before_coverage: number;
  questions_missing_start: number;
  negative_intervals: number;
}

export interface Health {
  range: HealthRange;
  metric_version: number;
  /** Earliest recorded event; null when nothing was recorded. */
  coverage_start: string | null;
  partial: boolean;
  computed_at: string;
  baseline: { taken_at: string | null };
  metrics: HealthMetric[];
  data_quality: HealthDataQuality;
}

export interface HealthDrillItem {
  task_id: string;
  title: string;
  title_origin: string;
  /** Self-reported status. */
  status: string;
  basis: Record<string, unknown>;
  href: string;
}

export interface HealthDrillPage {
  range: HealthRange;
  metric: string;
  metric_version: number;
  coverage_start: string | null;
  partial: boolean;
  computed_at: string;
  items: HealthDrillItem[];
  next_cursor: string | null;
}

export interface BaselineReport {
  observed_at: string;
  added: { tasks: number; handoffs: number; questions: number };
  existed_before: boolean;
  note: string;
}

export const HEALTH_METRICS = [
  'blocked_tasks',
  'blocked_duration',
  'handoff_waiting',
  'open_questions',
  'stale_path_claims',
] as const;

export const collabHealth = {
  get: (owner: string, repo: string, opts: { from?: string; to?: string } = {}) =>
    request<Health>(`${base(owner, repo)}${qs({ from: opts.from, to: opts.to })}`),

  drill: (
    owner: string,
    repo: string,
    opts: { metric: string; from?: string; to?: string; limit?: number; cursor?: string | null },
  ) =>
    request<HealthDrillPage>(
      `${base(owner, repo)}/tasks${qs({
        metric: opts.metric,
        from: opts.from,
        to: opts.to,
        limit: opts.limit,
        cursor: opts.cursor,
      })}`,
    ),

  takeBaseline: (owner: string, repo: string) =>
    request<BaselineReport>(`${base(owner, repo)}/baseline`, { method: 'POST' }),
};
