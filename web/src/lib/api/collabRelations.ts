// Same-account multi-agent collaboration — the "relations" slice of the human
// REST client: task dependencies (add / remove), user reassign, takeover
// history, budgets and usage.
//
// Like `collaboration.ts`, nothing here sends session credentials
// (`X-Ironforge-Session` / `-Secret`): every call is made as the signed-in
// account, which the server records as the `user` door. A `user` door is not
// proof that a person (rather than a client under that account) acted.
//
// Every text field (titles, reasons, units, sources, session names) is written
// by a client and must be rendered as plain text.
import { request, qs } from './_base.svelte';

const base = (owner: string, repo: string) =>
  `/repos/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/collaboration`;
const taskBase = (owner: string, repo: string, taskId: string) =>
  `${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}`;

// ── Dependencies ─────────────────────────────────────

export interface DependencyEdge {
  task_id: string;
  depends_on_task_id: string;
  created_by_kind: string;
  created_by_id: string;
  /** Graph version this edge's insertion produced. */
  graph_version: number;
  created_at: string;
  origin: string;
}

export interface AddDependencyInput {
  client_request_id: string;
  depends_on_task_id: string;
  /** The `graph_version` last read from the dependency list. */
  expected_graph_version: number;
}

export interface AddDependencyResult {
  dependency: DependencyEdge;
  /** Version after the change (the current one when `replayed`). */
  graph_version: number;
  replayed: boolean;
}

export interface RemoveDependencyInput {
  client_request_id: string;
  expected_graph_version: number;
}

export interface RemoveDependencyResult {
  /** `false` when the edge did not exist: already removed, still a success. */
  removed: boolean;
  graph_version: number;
}

/** The server refuses more direct dependencies than this per task. */
export const MAX_DIRECT_DEPENDENCIES = 100;

// ── Takeovers / reassign ─────────────────────────────

export interface ReassignInput {
  client_takeover_id: string;
  /** The task `version` the caller last saw. */
  expected_task_version: number;
  /** The owner the caller last saw; `null` for an unowned task. */
  expected_owner_session_id: string | null;
  to_session_id: string;
  /** 1 byte to 2000 bytes; a claim, not verified. */
  reason: string;
}

export interface TakeoverRecord {
  id: string;
  task_id: string;
  from_session_id: string | null;
  to_session_id: string;
  /** `session` or `user`: the door used, not proof of who used it. */
  actor_kind: string;
  actor_id: string;
  previous_task_version: number;
  new_task_version: number;
  /** `claimed` or `reassigned`. */
  outcome: string;
  reason: string;
  created_at: string;
  origin: string;
}

export interface TakeoverResult {
  task: { id: string; version: number; owner_session_id: string | null };
  takeover: TakeoverRecord;
  replayed: boolean;
}

export const MAX_REASON_BYTES = 2000;

// ── Budgets / usage ──────────────────────────────────

export interface UsageStatus {
  /** `unknown` (nothing reported for this unit and source) or `reported`. */
  status: string;
  reason: string | null;
  /** Sum of the reported records of exactly this unit and source; `null` (never `"0"`) when nothing was reported. */
  total: string | null;
  record_count: number;
  covered_from: string | null;
  covered_to: string | null;
  last_reported_at: string | null;
  overlapping_periods: boolean;
}

export interface ThresholdEvaluation {
  /** `alert_at` or `amount`: the number used as the threshold. */
  basis: string;
  threshold_amount: string;
  used_total: string;
  unit: string;
  source: string;
  comparison: string;
  total_may_double_count: boolean;
}

export interface BudgetEntry {
  task_id: string;
  unit: string;
  source: string;
  amount: string;
  alert_at: string | null;
  sampling_period: string;
  /** Send back as `expected_version` when replacing this budget. */
  version: number;
  set_by_kind: string;
  set_by_id: string;
  updated_at: string;
  usage: UsageStatus;
  over_threshold: boolean | null;
  threshold: ThresholdEvaluation | null;
  enforcement: string;
  origin: string;
}

export interface UnmatchedUsage {
  unit: string;
  source: string;
  total: string;
  record_count: number;
  covered_from: string;
  covered_to: string;
  overlapping_periods: boolean;
  note: string;
}

export interface BudgetSummary {
  task_id: string;
  as_of: string;
  budgets: BudgetEntry[];
  unmatched_usage: UnmatchedUsage[];
  enforcement: string;
  notes: string[];
}

export interface SetBudgetInput {
  /** The budget's `version` for this unit and source; 0 when none exists. */
  expected_version: number;
  unit: string;
  source: string;
  /** Decimal text, > 0, at most 18 integer and 6 fractional digits. */
  amount: string;
  alert_at?: string | null;
  sampling_period: string;
}

export interface ReportUsageInput {
  client_usage_id: string;
  unit: string;
  source: string;
  /** Decimal text: the amount consumed during the period (a delta). */
  amount: string;
  period_start: string;
  period_end: string;
}

export interface UsageRecord {
  id: string;
  task_id: string;
  unit: string;
  source: string;
  amount: string;
  period_start: string;
  period_end: string;
  reported_by_kind: string;
  reported_by_id: string;
  client_usage_id: string;
  created_at: string;
  origin: string;
}

export interface CursorPage<T> {
  items: T[];
  next_cursor: string | null;
}

// ── Client ───────────────────────────────────────────

export const collabRelations = {
  /** Add "task waits for depends_on". A stale `expected_graph_version` is a 409 `VERSION_CONFLICT`. */
  addDependency: (owner: string, repo: string, taskId: string, input: AddDependencyInput) =>
    request<AddDependencyResult>(`${taskBase(owner, repo, taskId)}/dependencies`, {
      method: 'POST',
      body: JSON.stringify(input),
    }),

  /** Idempotent: removing an edge that is gone answers `removed: false`. */
  removeDependency: (
    owner: string,
    repo: string,
    taskId: string,
    dependsOnId: string,
    input: RemoveDependencyInput,
  ) =>
    request<RemoveDependencyResult>(
      `${taskBase(owner, repo, taskId)}/dependencies/${encodeURIComponent(dependsOnId)}`,
      { method: 'DELETE', body: JSON.stringify(input) },
    ),

  /** A user moves the declared owner to a live session. Needs write access. Handoffs are untouched. */
  reassignTask: (owner: string, repo: string, taskId: string, input: ReassignInput) =>
    request<TakeoverResult>(`${taskBase(owner, repo, taskId)}/reassign`, {
      method: 'POST',
      body: JSON.stringify(input),
    }),

  /** Ownership changes, oldest first. Page with `next_cursor`. */
  listTakeovers: (
    owner: string,
    repo: string,
    taskId: string,
    opts: { limit?: number; cursor?: string | null } = {},
  ) =>
    request<CursorPage<TakeoverRecord>>(
      `${taskBase(owner, repo, taskId)}/takeovers${qs({ limit: opts.limit, cursor: opts.cursor })}`,
    ),

  getBudget: (owner: string, repo: string, taskId: string) =>
    request<BudgetSummary>(`${taskBase(owner, repo, taskId)}/budget`),

  /** Create or replace the budget of one exact unit and source (needs write access). */
  setBudget: (owner: string, repo: string, taskId: string, input: SetBudgetInput) =>
    request<BudgetEntry>(`${taskBase(owner, repo, taskId)}/budgets`, {
      method: 'PUT',
      body: JSON.stringify(input),
    }),

  /** Append a usage record as the signed-in account (a claim, not a measurement). Needs write access. */
  reportUsage: (owner: string, repo: string, taskId: string, input: ReportUsageInput) =>
    request<UsageRecord>(`${taskBase(owner, repo, taskId)}/usage`, {
      method: 'POST',
      body: JSON.stringify(input),
    }),

  /** Reported usage, newest reported first. */
  listUsage: (
    owner: string,
    repo: string,
    taskId: string,
    opts: { unit?: string; source?: string; limit?: number; cursor?: string | null } = {},
  ) =>
    request<CursorPage<UsageRecord>>(
      `${taskBase(owner, repo, taskId)}/usage${qs({ unit: opts.unit, source: opts.source, limit: opts.limit, cursor: opts.cursor })}`,
    ),
};
