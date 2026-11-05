// Same-account multi-agent collaboration — human-facing REST client.
//
// Only endpoints a signed-in person can use WITHOUT session credentials
// (`X-Ironforge-Session` / `-Secret`) are exposed here. Task creation/update,
// messages, handoffs and notices need a session secret and therefore have no
// client function on purpose.
//
// Every text field below (titles, bodies, branches, paths, labels) is written
// by a client — a person's tool or an agent — and must be rendered as plain
// text. Status fields are declarations, not evidence.
import { request, qs } from './_base.svelte';

const base = (owner: string, repo: string) =>
  `/repos/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/collaboration`;

// ── Types ────────────────────────────────────────────

/** Declared task status. Unknown future values must still render (as raw text). */
export type TaskStatus = 'open' | 'in_progress' | 'waiting' | 'blocked' | 'done';
export const TASK_STATUSES: readonly TaskStatus[] = ['open', 'in_progress', 'waiting', 'blocked', 'done'];

export interface CollabTask {
  id: string;
  title: string;
  description: string | null;
  status: string;
  owner_session_id: string | null;
  branch: string | null;
  paths: string[];
  paths_declared_at: string | null;
  issue_number: number | null;
  pr_number: number | null;
  version: number;
  last_message_seq: number;
  created_at: string;
  updated_at: string;
  origin: string;
}

export interface CursorPage<T> {
  items: T[];
  next_cursor: string | null;
}

export interface CollabSession {
  id: string;
  display_name: string;
  client_kind: string;
  /** `online` | `offline` | `expired` | `ended` */
  state: string;
  created_at: string;
  last_seen_at: string;
  expires_at: string;
  ended_at: string | null;
  origin: string;
}

export interface CollabMessage {
  id: string;
  task_id: string;
  seq: number;
  author_session_id: string;
  /** `note` | `help` | `blocker` */
  kind: string;
  body: string;
  reply_to_id: string | null;
  target_session_id: string | null;
  commit_sha: string | null;
  client_message_id: string;
  created_at: string;
  origin: string;
}

export interface MessagePage {
  items: CollabMessage[];
  next_after_seq: number;
  has_more: boolean;
}

export interface CollabHandoff {
  id: string;
  task_id: string;
  from_session_id: string;
  to_session_id: string;
  summary: string;
  completed: string;
  next_steps: string;
  blockers: string;
  commit_sha: string | null;
  evidence: string[];
  client_handoff_id: string;
  status: string;
  version: number;
  created_at: string;
  accepted_at: string | null;
  origin: string;
}

export interface QuestionOption {
  id: string;
  label: string;
}

export interface QuestionAnswer {
  id: string;
  actor_kind: string;
  actor_id: string;
  body: string;
  option_id: string | null;
  context_hash: string;
  created_at: string;
  origin: string;
}

export interface CollabQuestion {
  id: string;
  task_id: string;
  /** `user` | `session` — the door used, not proof of who is behind it. */
  creator_kind: string;
  creator_id: string;
  recipient: { kind: string; id: string | null };
  title: string;
  body: string;
  options: QuestionOption[];
  context: { task_version: number; base_sha: string | null; head_sha: string | null };
  /** Send back as `acknowledged_context_hash` when answering. */
  context_hash: string;
  /** `open` | `answered` | `cancelled` */
  status: string;
  /** Send back as `expected_version`. */
  version: number;
  client_question_id: string;
  created_at: string;
  updated_at: string;
  /** Present on single-question reads only. */
  answer: QuestionAnswer | null;
  /** `fresh` | `stale` | `unknown`; single-question reads only. */
  context_freshness: string | null;
  origin: string;
}

export interface AnswerInput {
  client_answer_id: string;
  expected_version: number;
  body: string;
  option_id?: string | null;
  acknowledged_context_hash: string;
}

export interface DecisionChange {
  id: string;
  decision_id: string;
  kind: string;
  replacement_decision_id: string | null;
  reason: string;
  actor_kind: string;
  actor_id: string;
  created_at: string;
  origin: string;
}

export interface CollabDecision {
  id: string;
  task_id: string;
  sequence: number;
  question_id: string | null;
  answer_id: string | null;
  title: string;
  rationale: string;
  context: { task_version: number; base_sha: string | null; head_sha: string | null };
  actor_kind: string;
  actor_id: string;
  created_at: string;
  /** `active` | `superseded` | `revoked` */
  status: string;
  change: DecisionChange | null;
  /** `fresh` | `stale` | `unknown` */
  context_freshness: string;
  origin: string;
}

export interface DependencyItem {
  task_id: string;
  depends_on_task_id: string;
  created_by_kind: string;
  graph_version: number;
  created_at: string;
  title: string;
  /** Declared by the dependency task's participants; not verified. */
  status: string;
  task_version: number;
  task_updated_at: string;
}

export interface DependencyList {
  task_id: string;
  graph_version: number;
  dependencies: DependencyItem[];
  /** Ids whose DECLARED status is not `done`. A declaration, not evidence. */
  blocked_by: string[];
  status_source: string;
  origin: string;
}

export interface PipelineEvidence {
  verification: string;
  pipeline_id: number;
  status: string | null;
  commit_sha: string | null;
  ref_name: string | null;
  finished_at: string | null;
  resolved_at: string;
}

export interface CompletionCondition {
  id: string;
  /** `satisfied` | `unsatisfied` | `stale` | `unknown` */
  status: string;
  detail: string;
  evidence: PipelineEvidence[];
}

export interface CompletionStatus {
  task_id: string;
  task_version: number;
  declared_status: string;
  conditions: CompletionCondition[];
  /** `all_satisfied` | `unsatisfied` | `incomplete` */
  overall: string;
  declared_done_without_evidence: boolean;
  observed_branch_head: string | null;
  evaluated_at: string;
  /** The conditions someone defined for this task, judged now; null when none are defined. */
  defined: DefinedStatus | null;
}

/** `question_answered` | `work_package_present` | `ci_passed_for_commit` | `manual_confirmation` (a newer server may add more). */
export interface DefinedConditionSpec {
  id: string;
  type: string;
  label: string;
  params: Record<string, unknown>;
  introduced_in_version: number;
  created_by_kind: string;
  created_by_id: string;
}

export interface ConfirmationView {
  id: string;
  task_id: string;
  condition_id: string;
  condition_set_version: number;
  /** The task version and branch head the confirmer was looking at. */
  task_version: number;
  head_sha: string | null;
  note: string;
  /** The confirming account; not proof that a person (rather than a client under it) acted. */
  confirmed_by_user_id: number;
  created_at: string;
  origin: string;
}

export interface DefinedCondition extends DefinedConditionSpec {
  /** `satisfied` | `unsatisfied` | `stale` | `unknown` */
  status: string;
  detail: string;
  evidence: { pipeline_id: number; status: string | null; commit_sha: string | null }[];
  confirmation: ConfirmationView | null;
}

export interface DefinedStatus {
  condition_set_version: number;
  conditions: DefinedCondition[];
  /** `all_satisfied` | `unsatisfied` | `incomplete` */
  overall: string;
}

export interface ConfirmationCreated {
  confirmation: ConfirmationView;
  replayed: boolean;
}

export interface SnapshotSummary {
  id: string;
  task_id: string;
  /** 0 = no defined conditions. */
  condition_set_version: number;
  task_version: number;
  overall: string;
  recorded_by_kind: string;
  recorded_by_id: string;
  created_at: string;
}

/** The evaluation exactly as recorded then; it is never recomputed. */
export interface SnapshotView extends SnapshotSummary {
  result: CompletionStatus;
  immutable: boolean;
  replayed: boolean;
}

export interface PathOverlap {
  mine: string;
  theirs: string;
  /** `equal` | `mine_contains_theirs` | `theirs_contains_mine` */
  reason: string;
}

export interface OverlapItem {
  task_id: string;
  status: string;
  owner_session_id: string | null;
  owner_state: string | null;
  branch: string | null;
  overlaps: PathOverlap[];
  overlaps_truncated: boolean;
  task_version: number;
  declared_at: string | null;
  /** `declared_current` | `declaration_stale` */
  confidence: string;
  origin: string;
}

export interface OverlapPage {
  items: OverlapItem[];
  next_cursor: string | null;
  as_of: string;
  /** Always `hint`. */
  authority: string;
  not_covered: string[];
}

export interface InboxItem {
  item_key: string;
  /** `assigned_task` | `pending_handoff` | `open_question` | `question_answer` */
  type: string;
  task_id: string;
  object_id: string;
  occurred_at: string;
  state: string;
  title: string;
  origin: string;
}

export interface Inbox {
  items: InboxItem[];
  as_of: string;
  truncated: boolean;
  consistency: string;
}

export interface OverviewItem {
  object_type: string;
  object_id: string;
  task_id: string;
  title: string;
  state: string;
  occurred_at: string;
  session_id: string | null;
  session_state: string | null;
  flags: string[];
  origin: string;
}

export interface OverviewSection {
  /** `awaiting_me` | `blocked_waiting` | `pending_handoffs` | `in_progress` */
  name: string;
  available: boolean;
  reason: string | null;
  items: OverviewItem[];
  as_of: string;
  truncated: boolean;
  continue_with: string;
}

export interface Overview {
  sections: OverviewSection[];
  as_of: string;
  consistency: string;
  notes: string[];
}

export interface DiagnosticFinding {
  code: string;
  /** `established` | `unknown` */
  confidence: string;
  /** `info` | `attention` */
  severity: string;
  statement: string;
  source: { record: string; id: string | null; fields: string[] };
  details: unknown;
}

export interface Diagnosis {
  subject: unknown;
  as_of: string;
  findings: DiagnosticFinding[];
  not_covered: string[];
  authority: string;
}

// ── Client ───────────────────────────────────────────

export const collaboration = {
  /** Tasks, most recently updated first. Page with `next_cursor`. */
  listTasks: (
    owner: string,
    repo: string,
    opts: { status?: string; limit?: number; cursor?: string | null } = {},
  ) =>
    request<CursorPage<CollabTask>>(
      `${base(owner, repo)}/tasks${qs({ status: opts.status, limit: opts.limit, cursor: opts.cursor })}`,
    ),

  getTask: (owner: string, repo: string, taskId: string) =>
    request<CollabTask>(`${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}`),

  completionStatus: (owner: string, repo: string, taskId: string) =>
    request<CompletionStatus>(`${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}/completion-status`),

  listDependencies: (owner: string, repo: string, taskId: string) =>
    request<DependencyList>(`${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}/dependencies`),

  listOverlaps: (
    owner: string,
    repo: string,
    taskId: string,
    opts: { limit?: number; cursor?: string | null } = {},
  ) =>
    request<OverlapPage>(
      `${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}/overlaps${qs({ limit: opts.limit, cursor: opts.cursor })}`,
    ),

  listMessages: (
    owner: string,
    repo: string,
    taskId: string,
    opts: { afterSeq?: number; limit?: number } = {},
  ) =>
    request<MessagePage>(
      `${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}/messages${qs({ after_seq: opts.afterSeq, limit: opts.limit })}`,
    ),

  listHandoffs: (
    owner: string,
    repo: string,
    taskId: string,
    opts: { limit?: number; cursor?: string | null } = {},
  ) =>
    request<CursorPage<CollabHandoff>>(
      `${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}/handoffs${qs({ limit: opts.limit, cursor: opts.cursor })}`,
    ),

  listDecisions: (
    owner: string,
    repo: string,
    taskId: string,
    opts: { limit?: number; cursor?: string | null } = {},
  ) =>
    request<CursorPage<CollabDecision>>(
      `${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}/decisions${qs({ limit: opts.limit, cursor: opts.cursor })}`,
    ),

  listQuestions: (
    owner: string,
    repo: string,
    taskId: string,
    opts: { status?: string; limit?: number; cursor?: string | null } = {},
  ) =>
    request<CursorPage<CollabQuestion>>(
      `${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}/questions${qs({ status: opts.status, limit: opts.limit, cursor: opts.cursor })}`,
    ),

  /** Single read: includes the answer, `context_hash`, `version` and freshness. */
  getQuestion: (owner: string, repo: string, questionId: string) =>
    request<CollabQuestion>(`${base(owner, repo)}/questions/${encodeURIComponent(questionId)}`),

  /** Answer with the signed-in account (no session headers). Needs write access. */
  answerQuestion: (owner: string, repo: string, questionId: string, input: AnswerInput) =>
    request<CollabQuestion>(`${base(owner, repo)}/questions/${encodeURIComponent(questionId)}/answer`, {
      method: 'POST',
      body: JSON.stringify(input),
    }),

  /** Only the creator may cancel; the server answers 403 for anyone else. */
  cancelQuestion: (owner: string, repo: string, questionId: string, expectedVersion: number) =>
    request<CollabQuestion>(`${base(owner, repo)}/questions/${encodeURIComponent(questionId)}/cancel`, {
      method: 'POST',
      body: JSON.stringify({ expected_version: expectedVersion }),
    }),

  /** Confirm a `manual_confirmation` condition with the signed-in account (needs write access). Not a merge approval. */
  confirmCondition: (
    owner: string,
    repo: string,
    taskId: string,
    conditionId: string,
    input: { client_request_id: string; note?: string },
  ) =>
    request<ConfirmationCreated>(
      `${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}/completion-conditions/${encodeURIComponent(conditionId)}/confirmations`,
      { method: 'POST', body: JSON.stringify(input) },
    ),

  /** Newest first; no cursor (at most `limit`). */
  listSnapshots: (owner: string, repo: string, taskId: string, opts: { limit?: number } = {}) =>
    request<{ items: SnapshotSummary[] }>(
      `${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}/completion-status/snapshots${qs({ limit: opts.limit })}`,
    ),

  getSnapshot: (owner: string, repo: string, taskId: string, snapshotId: string) =>
    request<SnapshotView>(
      `${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}/completion-status/snapshots/${encodeURIComponent(snapshotId)}`,
    ),

  /**
   * Store the current evaluation unchanged as a snapshot. With a `client_request_id`
   * a retry returns the snapshot that key already made.
   */
  createSnapshot: (owner: string, repo: string, taskId: string, clientRequestId: string) =>
    request<SnapshotView>(
      `${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}/completion-status/snapshots${qs({ client_request_id: clientRequestId })}`,
      { method: 'POST' },
    ),

  /** Questions waiting for the signed-in account. Needs write access (403 otherwise). */
  userInbox: (owner: string, repo: string, opts: { limit?: number } = {}) =>
    request<Inbox>(`${base(owner, repo)}/inbox/user${qs({ limit: opts.limit })}`),

  overview: (owner: string, repo: string, opts: { sections?: string[]; limit?: number } = {}) =>
    request<Overview>(
      `${base(owner, repo)}/overview${qs({ sections: opts.sections?.join(','), limit: opts.limit })}`,
    ),

  /** Read-only explanation derived from stored records; not a statement about fault. */
  diagnoseTask: (owner: string, repo: string, taskId: string) =>
    request<Diagnosis>(`${base(owner, repo)}/diagnostics${qs({ task_id: taskId })}`),

  /** Sessions (self-reported labels). Needs write access (403 otherwise). */
  listSessions: (owner: string, repo: string, opts: { limit?: number; cursor?: string | null } = {}) =>
    request<CursorPage<CollabSession>>(
      `${base(owner, repo)}/sessions${qs({ limit: opts.limit, cursor: opts.cursor })}`,
    ),
};

/**
 * Look up self-reported session labels by id (up to `maxPages` × 100 sessions,
 * most recently seen first). Returns `null` when the caller may not list
 * sessions (listing needs write access), so the UI can say "unavailable"
 * instead of guessing a state.
 */
export async function loadSessionMap(
  owner: string,
  repo: string,
  maxPages = 3,
): Promise<Map<string, CollabSession> | null> {
  const map = new Map<string, CollabSession>();
  let cursor: string | null = null;
  try {
    for (let i = 0; i < maxPages; i++) {
      const page: CursorPage<CollabSession> = await collaboration.listSessions(owner, repo, {
        limit: 100,
        cursor,
      });
      for (const s of page.items) map.set(s.id, s);
      if (!page.next_cursor) break;
      cursor = page.next_cursor;
    }
  } catch (e) {
    if (e instanceof Error && 'status' in e && ((e as { status: number }).status === 403 || (e as { status: number }).status === 401)) {
      return null;
    }
    throw e;
  }
  return map;
}
