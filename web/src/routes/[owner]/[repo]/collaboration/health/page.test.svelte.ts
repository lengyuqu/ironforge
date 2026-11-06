import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@testing-library/svelte';
import { ApiError } from '$lib/api/_base.svelte';

const pageStore = vi.hoisted(() => {
  const value = { params: { owner: 'alice', repo: 'demo' } };
  return {
    page: {
      subscribe(fn: (v: typeof value) => void) {
        fn(value);
        return () => {};
      },
    },
  };
});
vi.mock('$app/stores', () => ({ page: pageStore.page }));

const api = vi.hoisted(() => ({ get: vi.fn(), drill: vi.fn(), takeBaseline: vi.fn() }));
vi.mock('$lib/api/collabHealth', async (orig) => ({
  ...(await orig<typeof import('$lib/api/collabHealth')>()),
  collabHealth: {
    get: (...a: unknown[]) => api.get(...a),
    drill: (...a: unknown[]) => api.drill(...a),
    takeBaseline: (...a: unknown[]) => api.takeBaseline(...a),
  },
}));
const confirm = vi.hoisted(() => vi.fn());
vi.mock('$lib/components/confirm.svelte', () => ({ confirmDialog: (...a: unknown[]) => confirm(...a) }));
vi.mock('$lib/components/RepoHeader.svelte', async () => ({ default: (await import('../../../../../test-stubs/EmptyStub.svelte')).default }));
vi.mock('$lib/components/collaboration/CollabNav.svelte', async () => ({ default: (await import('../../../../../test-stubs/EmptyStub.svelte')).default }));
vi.mock('$lib/i18n', () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === 'object' ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

import Page from './+page.svelte';
import type { Health, HealthMetric } from '$lib/api/collabHealth';

const quality = {
  unknown_state_tasks: 0, inconsistent_tasks: 0, unobserved_pending_handoffs: 0, handoffs_closed_before_coverage: 0,
  handoffs_missing_start: 0, unobserved_open_questions: 0, questions_closed_before_coverage: 0, questions_missing_start: 0,
  negative_intervals: 0,
};

function metric(over: Partial<HealthMetric> & { id: string }): HealthMetric {
  return {
    definition_version: 1, source: 'user_content', timing_source: 'platform_event', status: 'complete',
    value: null, reason: null, notes: [], drill_down: [], ...over,
  };
}

function health(over: Partial<Health> = {}): Health {
  return {
    range: { from: '2026-11-01T00:00:00Z', to: '2026-11-08T00:00:00Z', timezone: 'UTC', interval: '[from, to)' },
    metric_version: 1,
    coverage_start: '2026-11-04T00:00:00Z',
    partial: true,
    computed_at: '2026-11-08T00:00:00Z',
    baseline: { taken_at: null },
    metrics: [
      metric({
        id: 'blocked_tasks', status: 'partial',
        value: { blocked_at_to: 0, entered_blocked_in_window: 2, unknown_state_tasks: 3, unit: 'tasks' },
        drill_down: [{ metric: 'blocked_at_to', href: '' }],
      }),
      metric({
        id: 'blocked_duration', status: 'complete', value: { blocked_seconds: 3725, intervals: 1, unit: 'seconds' },
      }),
      metric({ id: 'stale_path_claims', status: 'unknown', reason: 'PATH_DECLARED_AT_NOT_RECORDED' }),
    ],
    data_quality: { ...quality, unknown_state_tasks: 3 },
    ...over,
  };
}

const article = (id: string) => document.querySelector(`[data-metric="${id}"]`) as HTMLElement;

describe('collaboration health page', () => {
  beforeEach(() => {
    Object.values(api).forEach((f) => f.mockReset());
    confirm.mockReset();
  });

  it('shows an unknown metric as unknown, never as zero', async () => {
    api.get.mockResolvedValue(health());
    render(Page);
    await screen.findAllByTestId('metric');
    const unknown = article('stale_path_claims');
    expect(within(unknown).getByTestId('metric-unknown')).toBeInTheDocument();
    expect(unknown.textContent).not.toMatch(/\d/);
    expect(unknown.querySelector('dl')).toBeNull();
  });

  it('marks a partial metric as a lower bound and does not show a partial zero as 0', async () => {
    api.get.mockResolvedValue(health());
    render(Page);
    await screen.findAllByTestId('metric');
    const blocked = article('blocked_tasks');
    const zero = within(blocked).getByTestId('value-blocked_at_to');
    expect(zero).toHaveTextContent('collab_health.none_recorded');
    expect(zero.textContent).not.toMatch(/\d/);
    expect(within(blocked).getByTestId('value-entered_blocked_in_window')).toHaveTextContent('≥ 2');
    expect(blocked).toHaveTextContent('collab_health.lower_bound');
    // A complete metric carries no bound marker and formats seconds.
    const dur = within(article('blocked_duration')).getByTestId('value-blocked_seconds');
    expect(dur).toHaveTextContent('1h 2m 5s');
    expect(dur.textContent).not.toContain('≥');
  });

  it('shows a known server note in the UI language and an unknown one as sent', async () => {
    api.get.mockResolvedValue(
      health({
        metrics: [
          metric({
            id: 'blocked_tasks', status: 'partial', value: { blocked_at_to: 1, unit: 'tasks' },
            notes: [
              'The window starts before the first recorded event; only the covered part is measured.',
              'A sentence from a newer server.',
            ],
          }),
        ],
      }),
    );
    render(Page);
    await screen.findAllByTestId('metric');
    expect(screen.getByText('collab_health.note.window_before_coverage')).toBeInTheDocument();
    expect(screen.getByText('A sentence from a newer server.')).toBeInTheDocument();
  });

  it('labels the source of each figure and the coverage start, and warns when partial', async () => {
    api.get.mockResolvedValue(health());
    render(Page);
    await screen.findByTestId('partial-banner');
    expect(screen.getByTestId('coverage')).toHaveTextContent('collab_health.coverage.start');
    expect(article('blocked_tasks')).toHaveTextContent('collab_health.source.user_content');
    expect(screen.getByText('collab_health.no_ranking')).toBeInTheDocument();
  });

  it('with no recorded events: no coverage start, every metric unknown, no digits shown as values', async () => {
    api.get.mockResolvedValue(
      health({
        coverage_start: null,
        metrics: ['blocked_tasks', 'blocked_duration', 'handoff_waiting', 'open_questions'].map((id) =>
          metric({ id, status: 'unknown', reason: 'NO_EVENTS_BEFORE_WINDOW_END' }),
        ),
        data_quality: quality,
      }),
    );
    render(Page);
    await screen.findAllByTestId('metric');
    expect(screen.getAllByTestId('metric-unknown')).toHaveLength(4);
    expect(document.querySelectorAll('[data-testid^="value-"]')).toHaveLength(0);
    expect(screen.queryByTestId('data-quality')).toBeNull();
  });

  it('tells non-admins this is admins only and shows no figures', async () => {
    api.get.mockRejectedValue(new ApiError('admin required', 403));
    render(Page);
    expect(await screen.findByText('collab_health.admins_only')).toBeInTheDocument();
    expect(screen.queryByTestId('metric')).toBeNull();
    expect(screen.queryByRole('button', { name: 'collab_health.baseline.button' })).toBeNull();
  });

  it('asks for the window the person picked', async () => {
    api.get.mockResolvedValue(health());
    render(Page);
    await screen.findAllByTestId('metric');
    expect(api.get.mock.calls[0][2].from).toEqual(expect.any(String));
    const first = Date.parse(api.get.mock.calls[0][2].from);
    await fireEvent.change(screen.getByTestId('window-select'), { target: { value: '30' } });
    await waitFor(() => expect(api.get).toHaveBeenCalledTimes(2));
    expect(first - Date.parse(api.get.mock.calls[1][2].from)).toBeGreaterThan(20 * 86_400_000);
  });

  it('drills into tasks in id order, titles as inert text, statuses as declared', async () => {
    const hostile = '<img src=x onerror="window.__pwned=1">';
    api.get.mockResolvedValue(health());
    api.drill.mockResolvedValue({
      items: [{ task_id: 't1', title: hostile, title_origin: 'user_content', status: 'blocked', basis: { blocked_seconds_in_window: 120 }, href: '' }],
      next_cursor: null,
    });
    render(Page);
    await screen.findAllByTestId('metric');
    await fireEvent.click(within(article('blocked_tasks')).getByRole('button', { name: 'collab_health.drill_metric.blocked_at_to' }));
    const item = await screen.findByTestId('drill-item');
    expect(item).toHaveTextContent(hostile);
    expect(item).toHaveTextContent('2m');
    expect(document.querySelector('img')).toBeNull();
    expect(api.drill).toHaveBeenCalledWith('alice', 'demo', expect.objectContaining({
      metric: 'blocked_at_to', from: '2026-11-01T00:00:00Z', to: '2026-11-08T00:00:00Z',
    }));
    expect(screen.getByText(/collab_health.drill.note/)).toBeInTheDocument();
  });

  describe('baseline', () => {
    it('asks first and does nothing on cancel', async () => {
      api.get.mockResolvedValue(health());
      render(Page);
      await screen.findAllByTestId('metric');
      confirm.mockResolvedValue(false);
      await fireEvent.click(screen.getByRole('button', { name: 'collab_health.baseline.button' }));
      await waitFor(() => expect(confirm).toHaveBeenCalledTimes(1));
      expect(confirm.mock.calls[0][0].message).toBe('collab_health.baseline.confirm_message');
      expect(api.takeBaseline).not.toHaveBeenCalled();
    });

    it('records the baseline after confirmation, reports it and reloads', async () => {
      api.get.mockResolvedValue(health());
      api.takeBaseline.mockResolvedValue({
        observed_at: '2026-11-08T01:00:00Z', added: { tasks: 3, handoffs: 1, questions: 0 }, existed_before: false, note: 'Observation start only.',
      });
      render(Page);
      await screen.findAllByTestId('metric');
      confirm.mockResolvedValue(true);
      api.get.mockResolvedValue(health({ baseline: { taken_at: '2026-11-08T01:00:00Z' } }));
      await fireEvent.click(screen.getByRole('button', { name: 'collab_health.baseline.button' }));
      expect(await screen.findByTestId('baseline-report')).toHaveTextContent('Observation start only.');
      expect(api.takeBaseline).toHaveBeenCalledTimes(1);
      await waitFor(() => expect(api.get).toHaveBeenCalledTimes(2));
      expect(await screen.findByTestId('baseline-taken')).toBeInTheDocument();
    });

    it('shows the server error when it fails', async () => {
      api.get.mockResolvedValue(health());
      api.takeBaseline.mockRejectedValue(new ApiError('admin required', 403));
      render(Page);
      await screen.findAllByTestId('metric');
      confirm.mockResolvedValue(true);
      await fireEvent.click(screen.getByRole('button', { name: 'collab_health.baseline.button' }));
      await screen.findByText(/admin required/);
      expect(screen.queryByTestId('baseline-report')).toBeNull();
    });
  });
});
