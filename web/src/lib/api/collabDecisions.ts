// Collaboration REST client, part 2: decision writes, user-initiated questions
// and work packages (read-only).
//
// Like `collaboration.ts`, only endpoints a signed-in person can call WITHOUT
// session credentials (`X-Ironforge-Session` / `-Secret`) live here. Without
// those headers the server records the write under the user, not a session.
//
// Every text field is written by a client (a person's tool or an agent) and
// must be rendered as plain text. A decision is a record, not an approval; a
// work package body is the author's declaration, and only `evidence_resolution`
// is something the platform looked up itself.
import { request, qs } from './_base.svelte';
import type { CollabDecision, CollabQuestion, CursorPage } from './collaboration';

const base = (owner: string, repo: string) =>
  `/repos/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/collaboration`;

// ── Decisions ────────────────────────────────────────

export interface CreateDecisionInput {
  /** Idempotency key; a new one for every new attempt. */
  client_decision_id: string;
  /** 1–200 characters. */
  title: string;
  /** 1 byte – 16 KiB. The writer's own account; not verified. */
  rationale: string;
  question_id?: string | null;
  /** The answer of `question_id`; requires `question_id`. */
  answer_id?: string | null;
  /** `task_version` is a compare-and-set: it must equal the task's current version. */
  context?: { task_version?: number | null; base_sha?: string | null; head_sha?: string | null };
}

export type DecisionChangeKind = 'superseded' | 'revoked';

export interface ChangeDecisionInput {
  client_change_id: string;
  kind: DecisionChangeKind;
  /** Required for `superseded` (a later decision of the same task), forbidden for `revoked`. */
  replacement_decision_id?: string | null;
  /** 1 byte – 4 KiB. */
  reason: string;
}

/** A decision as returned by the write endpoints; `supersedes` is only on single reads. */
export interface DecisionRecord extends CollabDecision {
  client_decision_id: string;
  supersedes?: string[] | null;
}

// ── Work packages ────────────────────────────────────

export interface EvidenceRef {
  /** `pipeline` | `commit` | `url` | `note` — what the author claims it points at. */
  kind: string;
  reference: string;
}

/** The platform's own lookup of a pipeline evidence reference. */
export interface PipelineEvidence {
  /** `verified_reference` (found in this repository) | `unavailable`. */
  verification: string;
  pipeline_id: number;
  /** `pending` | `running` | `success` | `failed` | `canceled`; null when unavailable. */
  status: string | null;
  /** The commit the pipeline actually ran against. */
  commit_sha: string | null;
  ref_name: string | null;
  finished_at: string | null;
  resolved_at: string;
}

export interface WorkPackage {
  id: string;
  task_id: string;
  revision: number;
  author_kind: string;
  author_id: string;
  task_version: number;
  commit_sha: string | null;
  // The body below is the author's declaration.
  goal: string;
  scope: string;
  artifacts: string[];
  completed: string[];
  open_items: string[];
  next_steps: string[];
  evidence_refs: EvidenceRef[];
  /** What the author left out (`goal`, `commit_sha`, `next_steps`, `evidence_refs`). */
  missing_fields: string[];
  client_package_id: string;
  created_at: string;
  /** Single reads only; null in lists. */
  evidence_resolution: PipelineEvidence[] | null;
  origin: string;
}

// ── Questions (user door) ────────────────────────────

export type RecipientKind = 'repo_writers' | 'user' | 'session';

export interface CreateQuestionInput {
  client_question_id: string;
  recipient: { kind: RecipientKind; id?: string | null };
  /** 1–200 characters. */
  title: string;
  /** 1 byte – 16 KiB. */
  body: string;
  /** At most 10 labels; the server assigns ids `o1`, `o2`, … */
  options: string[];
  /** `task_version` is a compare-and-set against the task's current version. */
  context?: { task_version?: number | null };
}

// ── Client ───────────────────────────────────────────

export const collabDecisions = {
  /** Record a decision with the signed-in account (no session headers). Needs write access. Not an approval. */
  createDecision: (owner: string, repo: string, taskId: string, input: CreateDecisionInput) =>
    request<DecisionRecord>(`${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}/decisions`, {
      method: 'POST',
      body: JSON.stringify(input),
    }),

  /** Supersede or revoke a decision. Each decision accepts exactly one such change. */
  changeDecision: (owner: string, repo: string, decisionId: string, input: ChangeDecisionInput) =>
    request<DecisionRecord>(`${base(owner, repo)}/decisions/${encodeURIComponent(decisionId)}/changes`, {
      method: 'POST',
      body: JSON.stringify(input),
    }),

  /** Ask a question as the signed-in person (no session headers). Needs write access. */
  createQuestion: (owner: string, repo: string, taskId: string, input: CreateQuestionInput) =>
    request<CollabQuestion>(`${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}/questions`, {
      method: 'POST',
      body: JSON.stringify(input),
    }),

  /** Revisions of a task's work package, newest first. */
  listWorkPackages: (
    owner: string,
    repo: string,
    taskId: string,
    opts: { limit?: number; cursor?: string | null } = {},
  ) =>
    request<CursorPage<WorkPackage>>(
      `${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}/work-packages${qs({ limit: opts.limit, cursor: opts.cursor })}`,
    ),

  /** One revision with the platform's lookup of its pipeline evidence. */
  getWorkPackage: (owner: string, repo: string, packageId: string) =>
    request<WorkPackage>(`${base(owner, repo)}/work-packages/${encodeURIComponent(packageId)}`),
};
