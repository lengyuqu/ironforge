// Pure helpers for the decision / question / work-package panels. Nothing here
// touches the DOM, so the rules are unit-testable.
import type { CollabDecision } from '$lib/api/collaboration';
import type { PipelineEvidence, WorkPackage } from '$lib/api/collabDecisions';

export const MAX_TITLE_CHARS = 200;
export const MAX_RATIONALE_BYTES = 16 * 1024;
export const MAX_REASON_BYTES = 4 * 1024;
export const MAX_OPTIONS = 10;

const byteLength = (s: string) => new TextEncoder().encode(s).length;

/** Mirrors the server: a label is 1–200 characters once trimmed. */
export function titleProblem(title: string): 'required' | 'too_long' | null {
  const trimmed = title.trim();
  if (!trimmed) return 'required';
  return [...trimmed].length > MAX_TITLE_CHARS ? 'too_long' : null;
}

export function textProblem(text: string, maxBytes: number): 'required' | 'too_long' | null {
  if (!text.trim()) return 'required';
  return byteLength(text) > maxBytes ? 'too_long' : null;
}

/**
 * Decisions that may replace `target`: active, later in the task's sequence
 * (the server refuses anything not newer: REPLACEMENT_NOT_NEWER), and not the
 * target itself.
 */
export function replacementCandidates(target: Pick<CollabDecision, 'id' | 'sequence'>, all: CollabDecision[]): CollabDecision[] {
  return all.filter((d) => d.id !== target.id && d.sequence > target.sequence && d.status === 'active');
}

/** Error codes after which the decision list must be re-read before another attempt. */
const DECISION_RELOAD_CODES = new Set(['VERSION_CONFLICT', 'DECISION_ALREADY_CHANGED', 'DECISION_CYCLE', 'REPLACEMENT_NOT_NEWER']);
export function decisionNeedsReload(code: string | undefined): boolean {
  return code !== undefined && DECISION_RELOAD_CODES.has(code);
}

/** Option labels as the server will see them: trimmed, blanks dropped. */
export function cleanOptions(labels: string[]): string[] {
  return labels.map((l) => l.trim()).filter((l) => l !== '');
}

/** Why a question draft cannot be sent yet, or null. */
export function questionProblem(draft: {
  title: string;
  body: string;
  options: string[];
  recipientKind: string;
  recipientId: string;
}): string | null {
  const t = titleProblem(draft.title);
  if (t) return `title_${t}`;
  const b = textProblem(draft.body, MAX_RATIONALE_BYTES);
  if (b) return `body_${b}`;
  const opts = cleanOptions(draft.options);
  if (opts.length > MAX_OPTIONS) return 'too_many_options';
  if (opts.some((o) => [...o].length > MAX_TITLE_CHARS)) return 'option_too_long';
  if (draft.recipientKind === 'user' && !/^[1-9][0-9]*$/.test(draft.recipientId.trim())) return 'user_id';
  if (draft.recipientKind === 'session' && !draft.recipientId) return 'session_required';
  return null;
}

/**
 * Whether a pipeline the platform found ran on a different commit than the one
 * the package author declared. Only a comparison of two recorded values: it says
 * nothing about whether either is "right".
 */
export function commitDiffers(pkg: Pick<WorkPackage, 'commit_sha'>, ev: Pick<PipelineEvidence, 'verification' | 'commit_sha'>): boolean {
  return ev.verification === 'verified_reference' && !!pkg.commit_sha && !!ev.commit_sha && pkg.commit_sha !== ev.commit_sha;
}

export const KNOWN_VERIFICATION = ['verified_reference', 'unavailable'] as const;
export const KNOWN_PIPELINE_STATUS = ['pending', 'running', 'success', 'failed', 'canceled'] as const;
export const KNOWN_EVIDENCE_KIND = ['pipeline', 'commit', 'url', 'note'] as const;
export const KNOWN_MISSING = ['goal', 'commit_sha', 'next_steps', 'evidence_refs'] as const;
export const KNOWN_RECIPIENT = ['repo_writers', 'user', 'session'] as const;
