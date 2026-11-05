// Pure helpers for the collaboration UI. Nothing here touches the DOM, so the
// rules about who may do what are unit-testable.
import type { AnswerInput, CollabQuestion } from '$lib/api/collaboration';

/** Idempotency key for a write; generated once per form so a retry replays instead of duplicating. */
export function newClientId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return `c${Date.now().toString(36)}${Math.random().toString(36).slice(2, 12)}`;
}

/** First 8 characters of an opaque id, for compact display. */
export function shortId(id: string | null | undefined): string {
  return id ? id.slice(0, 8) : '';
}

/**
 * Only the creator may cancel a question, and only while it is open. A person
 * can therefore cancel the questions they asked themselves, never an agent's.
 */
export function canCancelQuestion(q: Pick<CollabQuestion, 'status' | 'creator_kind' | 'creator_id'>, userId: number | null | undefined): boolean {
  return userId != null && q.status === 'open' && q.creator_kind === 'user' && q.creator_id === String(userId);
}

/** An open question can be answered by a writer; the server decides about the recipient. */
export function canAnswerQuestion(q: Pick<CollabQuestion, 'status'>): boolean {
  return q.status === 'open';
}

export type AnswerDraft = { body: string; optionId: string | null; clientAnswerId: string };

/** Why a draft cannot be sent yet, or null when it can. Mirrors the server's rules. */
export function answerProblem(q: Pick<CollabQuestion, 'options'>, draft: Pick<AnswerDraft, 'body' | 'optionId'>): 'option_required' | 'body_required' | 'too_long' | null {
  if (new TextEncoder().encode(draft.body).length > 16 * 1024) return 'too_long';
  if (q.options.length > 0) return draft.optionId ? null : 'option_required';
  return draft.body.trim() ? null : 'body_required';
}

/** Build the answer request. It echoes the question's own `version` and `context_hash`, proving the context was seen. */
export function buildAnswer(q: Pick<CollabQuestion, 'version' | 'context_hash' | 'options'>, draft: AnswerDraft): AnswerInput {
  return {
    client_answer_id: draft.clientAnswerId,
    expected_version: q.version,
    body: draft.body,
    option_id: q.options.length > 0 ? draft.optionId : null,
    acknowledged_context_hash: q.context_hash,
  };
}

/** Error codes after which the question must be re-read before another attempt. */
const RELOAD_CODES = new Set(['VERSION_CONFLICT', 'CONTEXT_STALE', 'QUESTION_NOT_OPEN']);
export function needsReload(code: string | undefined): boolean {
  return code !== undefined && RELOAD_CODES.has(code);
}

/** Translation key for a known enum value under `prefix`, or null so callers can show the raw value. */
export function knownLabelKey(prefix: string, value: string | null | undefined, known: readonly string[]): string | null {
  return value && known.includes(value) ? `${prefix}.${value}` : null;
}

/** Local, human-readable timestamp; falls back to the raw text when it does not parse. */
export function formatTime(iso: string | null | undefined): string {
  if (!iso) return '';
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
}

/** Known enum values per field. Anything else is shown raw (a newer server may add values). */
export const KNOWN = {
  taskStatus: ['open', 'in_progress', 'waiting', 'blocked', 'done'],
  sessionState: ['online', 'offline', 'expired', 'ended', 'missing'],
  questionStatus: ['open', 'answered', 'cancelled'],
  freshness: ['fresh', 'stale', 'unknown'],
  decisionStatus: ['active', 'superseded', 'revoked'],
  handoffStatus: ['pending', 'accepted', 'cancelled'],
  messageKind: ['note', 'help', 'blocker'],
  conditionStatus: ['satisfied', 'unsatisfied', 'stale', 'unknown'],
  overall: ['all_satisfied', 'unsatisfied', 'incomplete'],
  confidence: ['declared_current', 'declaration_stale', 'established', 'unknown'],
  overlapReason: ['equal', 'mine_contains_theirs', 'theirs_contains_mine'],
  inboxType: ['assigned_task', 'pending_handoff', 'open_question', 'question_answer'],
  section: ['awaiting_me', 'blocked_waiting', 'pending_handoffs', 'in_progress'],
  flag: ['unowned', 'session_not_online'],
  severity: ['info', 'attention'],
  condition: ['ci_passed_for_commit', 'questions_resolved', 'work_package_present'],
} as const;
