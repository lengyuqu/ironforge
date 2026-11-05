import { describe, it, expect } from 'vitest';
import {
  answerProblem,
  buildAnswer,
  canAnswerQuestion,
  canCancelQuestion,
  knownLabelKey,
  needsReload,
  newClientId,
  shortId,
} from './collaboration';

const openUserQuestion = { status: 'open', creator_kind: 'user', creator_id: '7' };

describe('canCancelQuestion', () => {
  it('allows the user who asked while the question is open', () => {
    expect(canCancelQuestion(openUserQuestion, 7)).toBe(true);
  });
  it('refuses another user, an agent session, a closed question and a signed-out viewer', () => {
    expect(canCancelQuestion(openUserQuestion, 8)).toBe(false);
    expect(canCancelQuestion({ ...openUserQuestion, creator_kind: 'session', creator_id: '7' }, 7)).toBe(false);
    expect(canCancelQuestion({ ...openUserQuestion, status: 'answered' }, 7)).toBe(false);
    expect(canCancelQuestion(openUserQuestion, null)).toBe(false);
  });
});

describe('canAnswerQuestion', () => {
  it('is true only for open questions', () => {
    expect(canAnswerQuestion({ status: 'open' })).toBe(true);
    expect(canAnswerQuestion({ status: 'cancelled' })).toBe(false);
  });
});

describe('answerProblem / buildAnswer', () => {
  const free = { options: [], version: 3, context_hash: 'abc' };
  const choice = { options: [{ id: 'o1', label: 'Yes' }, { id: 'o2', label: 'No' }], version: 4, context_hash: 'def' };

  it('requires a non-blank body for free-text questions', () => {
    expect(answerProblem(free, { body: '   ', optionId: null })).toBe('body_required');
    expect(answerProblem(free, { body: 'go', optionId: null })).toBeNull();
  });
  it('requires an option when the question has options, body optional', () => {
    expect(answerProblem(choice, { body: 'because', optionId: null })).toBe('option_required');
    expect(answerProblem(choice, { body: '', optionId: 'o2' })).toBeNull();
  });
  it('rejects bodies over 16 KiB measured in bytes', () => {
    expect(answerProblem(free, { body: 'é'.repeat(8 * 1024 + 1), optionId: null })).toBe('too_long');
  });
  it('echoes the question version and context hash and drops option_id for free text', () => {
    expect(buildAnswer(free, { body: 'go', optionId: 'o9', clientAnswerId: 'k' })).toEqual({
      client_answer_id: 'k',
      expected_version: 3,
      body: 'go',
      option_id: null,
      acknowledged_context_hash: 'abc',
    });
    expect(buildAnswer(choice, { body: '', optionId: 'o1', clientAnswerId: 'k2' }).option_id).toBe('o1');
  });
});

describe('misc helpers', () => {
  it('needsReload recognises the stale-state codes only', () => {
    expect(needsReload('CONTEXT_STALE')).toBe(true);
    expect(needsReload('VERSION_CONFLICT')).toBe(true);
    expect(needsReload('SESSION_EXPIRED')).toBe(false);
    expect(needsReload(undefined)).toBe(false);
  });
  it('knownLabelKey returns null for unknown enum values', () => {
    expect(knownLabelKey('x.status', 'open', ['open', 'done'])).toBe('x.status.open');
    expect(knownLabelKey('x.status', 'weird', ['open', 'done'])).toBeNull();
    expect(knownLabelKey('x.status', null, ['open'])).toBeNull();
  });
  it('newClientId yields distinct, header-safe keys', () => {
    const a = newClientId();
    expect(a).toMatch(/^[A-Za-z0-9._:-]{1,128}$/);
    expect(newClientId()).not.toBe(a);
  });
  it('shortId truncates and tolerates null', () => {
    expect(shortId('0123456789abcdef')).toBe('01234567');
    expect(shortId(null)).toBe('');
  });
});
