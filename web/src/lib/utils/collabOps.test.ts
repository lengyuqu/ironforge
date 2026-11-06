import { describe, it, expect } from 'vitest';
import {
  buildEscalation,
  checklistToText,
  clockToMinute,
  draftFromSettings,
  linesOf,
  minuteToClock,
  placeholdersOf,
  sameContent,
  templateProblem,
  textToChecklist,
} from './collabOps';
import type { EscalationSettings } from '$lib/api/collabOps';

const content = (over = {}) => ({ title_pattern: 'Fix {bug}', goal: 'Fix it', deliverables: [], checklist: [], ...over });

describe('placeholdersOf', () => {
  it('lists distinct names in order', () => {
    expect(placeholdersOf('{a} and {b} and {a}')).toEqual(['a', 'b']);
    expect(placeholdersOf('plain')).toEqual([]);
  });
  it('rejects any brace that is not a well-formed placeholder', () => {
    for (const bad of ['{', '}', '{}', '{a b}', '{a', 'a}', '{{a}}', `{${'x'.repeat(33)}}`]) {
      expect(placeholdersOf(bad), bad).toBeNull();
    }
  });
});

describe('templateProblem', () => {
  it('accepts valid content', () => {
    expect(templateProblem(content(), 'Bugfix')).toBeNull();
  });
  it('checks the name only when given', () => {
    expect(templateProblem(content())).toBeNull();
    expect(templateProblem(content(), '  ')).toBe('name_required');
    expect(templateProblem(content(), 'x'.repeat(101))).toBe('name_too_long');
  });
  it('flags each rule the server enforces', () => {
    expect(templateProblem(content({ title_pattern: ' ' }))).toBe('title_required');
    expect(templateProblem(content({ title_pattern: 'a {' }))).toBe('title_braces');
    expect(templateProblem(content({ goal: '' }))).toBe('goal_required');
    expect(templateProblem(content({ goal: 'é'.repeat(1025) }))).toBe('goal_too_long');
    expect(templateProblem(content({ deliverables: Array(21).fill('x') }))).toBe('too_many_deliverables');
    expect(templateProblem(content({ checklist: Array(31).fill({ text: 'x', required: true }) }))).toBe('too_many_checklist');
    expect(templateProblem(content({ deliverables: ['x'.repeat(257)] }))).toBe('item_too_long');
    expect(templateProblem(content({ deliverables: ['a\nb'] }))).toBe('item_multiline');
    expect(templateProblem(content({ checklist: [{ text: ' ', required: true }] }))).toBe('checklist_empty_item');
  });
});

describe('text <-> list helpers', () => {
  it('splits lines, trimming and dropping blanks', () => {
    expect(linesOf(' a \r\n\n b\n')).toEqual(['a', 'b']);
  });
  it('round-trips the checklist with optional markers', () => {
    const items = [{ text: 'one', required: true }, { text: 'two', required: false }];
    expect(textToChecklist(checklistToText(items))).toEqual(items);
  });
  it('detects an unchanged draft', () => {
    expect(sameContent(content(), content())).toBe(true);
    expect(sameContent(content(), content({ goal: 'other' }))).toBe(false);
    expect(sameContent(content({ checklist: [{ text: 'a', required: true }] }), content({ checklist: [{ text: 'a', required: false }] }))).toBe(false);
  });
});

describe('escalation draft', () => {
  const settings: EscalationSettings = {
    enabled: true,
    blocked_after_secs: 14400,
    question_after_secs: 3600,
    handoff_after_secs: 900,
    repeat_after_secs: 0,
    quiet_hours: { start_minute: 1320, end_minute: 420 },
    timezone: 'UTC',
    version: 3,
    source: 'stored',
    updated_at: '2026-11-06T00:00:00Z',
    updated_by_user_id: 1,
  };

  it('converts clock and minutes both ways', () => {
    expect(minuteToClock(1320)).toBe('22:00');
    expect(clockToMinute('07:05')).toBe(425);
    expect(clockToMinute('24:00')).toBeNull();
    expect(clockToMinute('nope')).toBeNull();
  });

  it('builds the PUT body from the draft it came from, carrying the version the editor saw', () => {
    const built = buildEscalation(draftFromSettings(settings), 3);
    expect(built).toEqual({
      ok: true,
      input: {
        expected_version: 3,
        enabled: true,
        blocked_after_secs: 14400,
        question_after_secs: 3600,
        handoff_after_secs: 900,
        repeat_after_secs: 0,
        quiet_hours: { start_minute: 1320, end_minute: 420 },
      },
    });
  });

  it('rejects what the server would reject', () => {
    const base = draftFromSettings(settings);
    expect(buildEscalation({ ...base, blockedMinutes: '0' }, 1)).toMatchObject({ ok: false, problem: { field: 'blocked' } });
    expect(buildEscalation({ ...base, questionMinutes: '1.5' }, 1)).toMatchObject({ ok: false, problem: { field: 'question' } });
    expect(buildEscalation({ ...base, handoffMinutes: '43201' }, 1)).toMatchObject({ ok: false, problem: { field: 'handoff' } });
    expect(buildEscalation({ ...base, repeatMinutes: '3' }, 1)).toMatchObject({ ok: false, problem: { field: 'repeat' } });
    expect(buildEscalation({ ...base, quietStart: '08:00', quietEnd: '08:00' }, 1)).toMatchObject({ ok: false, problem: { field: 'quiet' } });
  });

  it('treats an empty repeat as once and no quiet window as null', () => {
    const built = buildEscalation({ ...draftFromSettings(settings), repeatMinutes: '', quiet: false }, 0);
    expect(built).toMatchObject({ ok: true, input: { repeat_after_secs: 0, quiet_hours: null } });
  });
});
