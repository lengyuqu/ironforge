import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import PipelineBadge from './PipelineBadge.svelte';

describe('PipelineBadge.svelte', () => {
  it('maps each terminal status to its icon and status key', () => {
    const cases: Array<[string, string]> = [
      ['success', '✓'],
      ['failed', '✗'],
      ['failure', '✗'],
      ['error', '✗'],
      ['manual', '▶'],
      ['waiting_approval', '◷'],
      ['canceled', '−'],
      ['skipped', '○'],
    ];
    for (const [status, icon] of cases) {
      const { unmount } = render(PipelineBadge, { status });
      // Icon and key share one span — assert on the badge's full text.
      const text = document.querySelector('.badge')?.textContent ?? '';
      expect(text).toContain(`pipeline.status.${status}`);
      expect(text).toContain(icon);
      unmount();
    }
  });

  it('renders a spinner (not an icon glyph) for the running status', () => {
    render(PipelineBadge, { status: 'running' });

    expect(document.querySelector('.badge')?.textContent).toContain('pipeline.status.running');
    expect(document.querySelector('.spinner')).toBeInTheDocument();
  });

  it('falls back to the pending look for unknown statuses', () => {
    render(PipelineBadge, { status: 'time-travel' });

    // Unknown key still flows through t(); look falls back to pending (●).
    expect(document.querySelector('.badge')?.textContent).toContain('●');
    expect(document.querySelector('.badge')?.getAttribute('style')).toContain('var(--yellow)');
  });

  it('colors success green and failed red', () => {
    const { unmount } = render(PipelineBadge, { status: 'success' });
    expect(document.querySelector('.badge')?.getAttribute('style')).toContain('var(--green)');
    unmount();

    render(PipelineBadge, { status: 'failed' });
    expect(document.querySelector('.badge')?.getAttribute('style')).toContain('var(--red)');
  });
});
