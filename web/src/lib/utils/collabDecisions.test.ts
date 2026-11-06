import { describe, it, expect } from 'vitest';
import { cleanOptions, commitDiffers, decisionNeedsReload, questionProblem, replacementCandidates, textProblem, titleProblem } from './collabDecisions';

describe('collabDecisions helpers', () => {
  it('validates titles like the server (trimmed, 1–200 characters)', () => {
    expect(titleProblem('   ')).toBe('required');
    expect(titleProblem('x'.repeat(200))).toBeNull();
    expect(titleProblem('x'.repeat(201))).toBe('too_long');
    expect(titleProblem('决'.repeat(200))).toBeNull();
  });
  it('limits text by bytes, not characters', () => {
    expect(textProblem('', 4096)).toBe('required');
    expect(textProblem('决'.repeat(1366), 4096)).toBe('too_long');
    expect(textProblem('a'.repeat(4096), 4096)).toBeNull();
  });
  it('offers only newer active decisions as replacements', () => {
    const d = (id: string, sequence: number, status = 'active') => ({ id, sequence, status }) as never;
    const all = [d('a', 1), d('b', 2), d('c', 3, 'revoked'), d('d', 4, 'superseded'), d('e', 5)];
    expect(replacementCandidates({ id: 'a', sequence: 1 }, all).map((x) => x.id)).toEqual(['b', 'e']);
    expect(replacementCandidates({ id: 'e', sequence: 5 }, all)).toEqual([]);
  });
  it('knows which codes need a reload', () => {
    expect(decisionNeedsReload('DECISION_ALREADY_CHANGED')).toBe(true);
    expect(decisionNeedsReload('VERSION_CONFLICT')).toBe(true);
    expect(decisionNeedsReload('IDEMPOTENCY_KEY_REUSED')).toBe(false);
    expect(decisionNeedsReload(undefined)).toBe(false);
  });
  it('checks question drafts', () => {
    const ok = { title: 'T', body: 'B', options: [], recipientKind: 'repo_writers', recipientId: '' };
    expect(questionProblem(ok)).toBeNull();
    expect(questionProblem({ ...ok, title: '' })).toBe('title_required');
    expect(questionProblem({ ...ok, body: ' ' })).toBe('body_required');
    expect(questionProblem({ ...ok, options: Array(11).fill('x') })).toBe('too_many_options');
    expect(questionProblem({ ...ok, options: Array(11).fill(' ') })).toBeNull();
    expect(questionProblem({ ...ok, recipientKind: 'user', recipientId: '0' })).toBe('user_id');
    expect(questionProblem({ ...ok, recipientKind: 'user', recipientId: '42' })).toBeNull();
    expect(questionProblem({ ...ok, recipientKind: 'session', recipientId: '' })).toBe('session_required');
    expect(cleanOptions([' a ', '', '  '])).toEqual(['a']);
  });
  it('compares the declared and the pipeline commit only for found pipelines', () => {
    expect(commitDiffers({ commit_sha: 'a' }, { verification: 'verified_reference', commit_sha: 'b' })).toBe(true);
    expect(commitDiffers({ commit_sha: 'a' }, { verification: 'verified_reference', commit_sha: 'a' })).toBe(false);
    expect(commitDiffers({ commit_sha: null }, { verification: 'verified_reference', commit_sha: 'b' })).toBe(false);
    expect(commitDiffers({ commit_sha: 'a' }, { verification: 'unavailable', commit_sha: null })).toBe(false);
  });
});
