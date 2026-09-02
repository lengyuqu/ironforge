import { describe, it, expect } from 'vitest';
import { formatDuration, statusIcon, statusColor, isRunning } from './pipelineStatus';

describe('pipelineStatus', () => {
  describe('formatDuration', () => {
    it('returns "-" without a start timestamp', () => {
      expect(formatDuration(null, '2026-09-02T10:00:00Z')).toBe('-');
      expect(formatDuration(undefined, undefined)).toBe('-');
    });

    it('formats seconds below one minute', () => {
      expect(formatDuration('2026-09-02T10:00:00Z', '2026-09-02T10:00:42Z')).toBe('42s');
    });

    it('formats minutes+seconds and hours+minutes', () => {
      expect(formatDuration('2026-09-02T10:00:00Z', '2026-09-02T10:01:42Z')).toBe('1m 42s');
      expect(formatDuration('2026-09-02T10:00:00Z', '2026-09-02T12:07:00Z')).toBe('2h 7m');
    });

    it('clamps negative durations to 0s (end before start)', () => {
      expect(formatDuration('2026-09-02T10:00:10Z', '2026-09-02T10:00:00Z')).toBe('0s');
    });
  });

  it('maps every status to a glyph and a colour', () => {
    const cases: Array<[string, string, string]> = [
      ['success', '✓', 'var(--green)'],
      ['failed', '✗', 'var(--red)'],
      ['running', '⟳', 'var(--accent)'],
      ['canceled', '−', 'var(--text-muted)'],
      ['pending', '●', 'var(--yellow)'],
    ];
    for (const [status, icon, color] of cases) {
      expect(statusIcon(status)).toBe(icon);
      expect(statusColor(status)).toBe(color);
    }
  });

  it('treats running and pending as in-flight', () => {
    expect(isRunning('running')).toBe(true);
    expect(isRunning('pending')).toBe(true);
    expect(isRunning('success')).toBe(false);
    expect(isRunning('')).toBe(false);
  });
});
