// Pure helpers for the collaboration ops pages (templates, escalation settings).
// They mirror the server's validation so a form can explain a problem before a
// round trip; the server still decides.
import type { ChecklistItem, EscalationInput, EscalationSettings, TemplateContent } from '$lib/api/collabOps';

export const LIMITS = {
  nameChars: 100,
  goalBytes: 2048,
  itemBytes: 256,
  deliverables: 20,
  checklist: 30,
  renderedTitleChars: 200,
  minThresholdSecs: 60,
  minRepeatSecs: 300,
  maxSecs: 30 * 24 * 3600,
} as const;

const byteLength = (s: string) => new TextEncoder().encode(s).length;

/** `{name}` placeholders of a title pattern in order of first appearance, or null when a brace is malformed. */
export function placeholdersOf(pattern: string): string[] | null {
  const names: string[] = [];
  let i = 0;
  while (i < pattern.length) {
    const ch = pattern[i];
    if (ch === '{') {
      const end = pattern.indexOf('}', i);
      if (end === -1) return null;
      const name = pattern.slice(i + 1, end);
      if (!/^[A-Za-z0-9_]{1,32}$/.test(name)) return null;
      if (!names.includes(name)) names.push(name);
      i = end + 1;
    } else if (ch === '}') {
      return null;
    } else {
      i += 1;
    }
  }
  return names;
}

export type TemplateProblem =
  | 'name_required'
  | 'name_too_long'
  | 'title_required'
  | 'title_braces'
  | 'goal_required'
  | 'goal_too_long'
  | 'item_too_long'
  | 'item_multiline'
  | 'too_many_deliverables'
  | 'too_many_checklist'
  | 'checklist_empty_item';

/** First problem the server would reject for this content, or null. `name` is checked only when given. */
export function templateProblem(content: TemplateContent, name?: string): TemplateProblem | null {
  if (name !== undefined) {
    const n = name.trim();
    if (!n) return 'name_required';
    if ([...n].length > LIMITS.nameChars) return 'name_too_long';
  }
  if (!content.title_pattern.trim()) return 'title_required';
  if (placeholdersOf(content.title_pattern) === null) return 'title_braces';
  if (!content.goal.trim()) return 'goal_required';
  if (byteLength(content.goal) > LIMITS.goalBytes) return 'goal_too_long';
  if (content.deliverables.length > LIMITS.deliverables) return 'too_many_deliverables';
  if (content.checklist.length > LIMITS.checklist) return 'too_many_checklist';
  for (const d of content.deliverables) {
    if (/[\r\n]/.test(d)) return 'item_multiline';
    if (byteLength(d) > LIMITS.itemBytes) return 'item_too_long';
  }
  for (const c of content.checklist) {
    if (!c.text.trim()) return 'checklist_empty_item';
    if (/[\r\n]/.test(c.text)) return 'item_multiline';
    if (byteLength(c.text) > LIMITS.itemBytes) return 'item_too_long';
  }
  return null;
}

/** Split a textarea into trimmed, non-empty lines. */
export function linesOf(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l !== '');
}

/** Checklist as one line per item; an item that is optional is written `[optional] text`. */
export function checklistToText(items: ChecklistItem[]): string {
  return items.map((i) => (i.required ? i.text : `[optional] ${i.text}`)).join('\n');
}

export function textToChecklist(text: string): ChecklistItem[] {
  return linesOf(text).map((line) =>
    line.startsWith('[optional] ') ? { text: line.slice('[optional] '.length).trim(), required: false } : { text: line, required: true },
  );
}

/** Whether two contents are identical (used to avoid saving an empty "new version"). */
export function sameContent(a: TemplateContent, b: TemplateContent): boolean {
  return (
    a.title_pattern === b.title_pattern &&
    a.goal === b.goal &&
    JSON.stringify(a.deliverables) === JSON.stringify(b.deliverables) &&
    JSON.stringify(a.checklist.map((c) => [c.text, c.required])) === JSON.stringify(b.checklist.map((c) => [c.text, c.required]))
  );
}

// ── escalation settings ──

export type EscalationDraft = {
  enabled: boolean;
  blockedMinutes: string;
  questionMinutes: string;
  handoffMinutes: string;
  /** '' or '0' = remind once per episode. */
  repeatMinutes: string;
  quiet: boolean;
  quietStart: string; // HH:MM, UTC
  quietEnd: string;
};

export function minuteToClock(minute: number): string {
  const m = Math.max(0, Math.min(1439, Math.trunc(minute)));
  return `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;
}

export function clockToMinute(clock: string): number | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(clock.trim());
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  return h < 24 && min < 60 ? h * 60 + min : null;
}

export function draftFromSettings(s: EscalationSettings): EscalationDraft {
  return {
    enabled: s.enabled,
    blockedMinutes: String(s.blocked_after_secs / 60),
    questionMinutes: String(s.question_after_secs / 60),
    handoffMinutes: String(s.handoff_after_secs / 60),
    repeatMinutes: String(s.repeat_after_secs / 60),
    quiet: s.quiet_hours !== null,
    quietStart: s.quiet_hours ? minuteToClock(s.quiet_hours.start_minute) : '22:00',
    quietEnd: s.quiet_hours ? minuteToClock(s.quiet_hours.end_minute) : '07:00',
  };
}

export type EscalationProblem =
  | { field: 'blocked' | 'question' | 'handoff'; code: 'threshold_range' }
  | { field: 'repeat'; code: 'repeat_range' }
  | { field: 'quiet'; code: 'quiet_invalid' };

const wholeMinutes = (text: string): number | null => {
  const t = text.trim();
  if (!/^\d+$/.test(t)) return null;
  return Number(t);
};

/** Build the PUT body from the draft, or the first problem the server would reject. */
export function buildEscalation(
  draft: EscalationDraft,
  expectedVersion: number,
): { ok: true; input: EscalationInput } | { ok: false; problem: EscalationProblem } {
  const thresholds: Array<['blocked' | 'question' | 'handoff', string]> = [
    ['blocked', draft.blockedMinutes],
    ['question', draft.questionMinutes],
    ['handoff', draft.handoffMinutes],
  ];
  const secs: Record<string, number> = {};
  for (const [field, text] of thresholds) {
    const m = wholeMinutes(text);
    const s = m === null ? NaN : m * 60;
    if (!(s >= LIMITS.minThresholdSecs && s <= LIMITS.maxSecs)) return { ok: false, problem: { field, code: 'threshold_range' } };
    secs[field] = s;
  }
  const repeatM = draft.repeatMinutes.trim() === '' ? 0 : wholeMinutes(draft.repeatMinutes);
  const repeat = repeatM === null ? NaN : repeatM * 60;
  if (!(repeat === 0 || (repeat >= LIMITS.minRepeatSecs && repeat <= LIMITS.maxSecs))) {
    return { ok: false, problem: { field: 'repeat', code: 'repeat_range' } };
  }
  let quiet: EscalationInput['quiet_hours'] = null;
  if (draft.quiet) {
    const start = clockToMinute(draft.quietStart);
    const end = clockToMinute(draft.quietEnd);
    if (start === null || end === null || start === end) return { ok: false, problem: { field: 'quiet', code: 'quiet_invalid' } };
    quiet = { start_minute: start, end_minute: end };
  }
  return {
    ok: true,
    input: {
      expected_version: expectedVersion,
      enabled: draft.enabled,
      blocked_after_secs: secs.blocked,
      question_after_secs: secs.question,
      handoff_after_secs: secs.handoff,
      repeat_after_secs: repeat,
      quiet_hours: quiet,
    },
  };
}

/** Error codes after which the stored copy must be re-read before another attempt. */
export function isVersionConflict(code: string | undefined): boolean {
  return code === 'VERSION_CONFLICT';
}
