import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { ApiError } from '$lib/api/_base.svelte';

const getBudget = vi.fn();
const setBudget = vi.fn();
const reportUsage = vi.fn();
const listUsage = vi.fn();
const toastSuccess = vi.fn();

vi.mock('$lib/api/collabRelations', async (importOriginal) => {
  const orig = await importOriginal<typeof import('$lib/api/collabRelations')>();
  return {
    ...orig,
    collabRelations: {
      getBudget: (...a: unknown[]) => getBudget(...a),
      setBudget: (...a: unknown[]) => setBudget(...a),
      reportUsage: (...a: unknown[]) => reportUsage(...a),
      listUsage: (...a: unknown[]) => listUsage(...a),
    },
  };
});
vi.mock('$lib/components/toast.svelte', () => ({ toast: { success: (...a: unknown[]) => toastSuccess(...a), error: vi.fn() } }));
vi.mock('$lib/i18n', () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === 'object' ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

import BudgetCard from './BudgetCard.svelte';
import type { BudgetEntry, BudgetSummary } from '$lib/api/collabRelations';

function budget(over: Partial<BudgetEntry> = {}): BudgetEntry {
  return {
    task_id: 't1', unit: 'USD', source: 'billing-api', amount: '25', alert_at: null, sampling_period: 'task_total', version: 2,
    set_by_kind: 'user', set_by_id: '7', updated_at: '2026-11-05T10:00:00Z',
    usage: {
      status: 'unknown', reason: 'nothing reported', total: null, record_count: 0, covered_from: null, covered_to: null,
      last_reported_at: null, overlapping_periods: false,
    },
    over_threshold: null, threshold: null, enforcement: 'none', origin: 'user_content', ...over,
  };
}

function summary(budgets: BudgetEntry[] = [], unmatched: BudgetSummary['unmatched_usage'] = []): BudgetSummary {
  return { task_id: 't1', as_of: '2026-11-05T11:00:00Z', budgets, unmatched_usage: unmatched, enforcement: 'none', notes: ['a', 'b', 'c'] };
}

function mount() {
  render(BudgetCard, { props: { owner: 'alice', repo: 'demo', taskId: 't1' } });
}

describe('BudgetCard.svelte', () => {
  beforeEach(() => {
    getBudget.mockReset().mockResolvedValue(summary());
    setBudget.mockReset();
    reportUsage.mockReset();
    listUsage.mockReset().mockResolvedValue({ items: [], next_cursor: null });
    toastSuccess.mockReset();
    delete (window as unknown as { __pwned?: number }).__pwned;
  });

  it('states the rules: units never mixed, unknown is not zero, nothing is enforced', async () => {
    mount();
    expect(await screen.findByTestId('budget-none')).toBeInTheDocument();
    expect(screen.getByText('collab_relations.budget.note_units')).toBeInTheDocument();
    expect(screen.getByText('collab_relations.budget.note_unknown')).toBeInTheDocument();
    expect(screen.getByText('collab_relations.budget.note_enforcement')).toBeInTheDocument();
  });

  it('shows missing usage as unknown — never as a number', async () => {
    getBudget.mockResolvedValue(summary([budget()]));
    mount();
    const usage = await screen.findByTestId('budget-usage');
    expect(usage.textContent).toContain('collab_relations.budget.usage_unknown');
    expect(usage.textContent).not.toContain('usage_total');
    expect(screen.getByTestId('budget-threshold').textContent).toContain('collab_relations.budget.not_judged');
  });

  it('keeps different units and sources apart and does not add them up', async () => {
    getBudget.mockResolvedValue(
      summary(
        [
          budget({
            unit: 'USD',
            usage: { status: 'reported', reason: null, total: '3.5', record_count: 2, covered_from: '2026-11-05T09:00:00Z', covered_to: '2026-11-05T10:00:00Z', last_reported_at: '2026-11-05T10:00:00Z', overlapping_periods: true },
            over_threshold: true,
            threshold: { basis: 'amount', threshold_amount: '25', used_total: '3.5', unit: 'USD', source: 'billing-api', comparison: 'used >= threshold', total_may_double_count: true },
          }),
          budget({ unit: 'tokens', source: 'llm', amount: '1000' }),
        ],
        [{ unit: 'EUR', source: 'x', total: '9', record_count: 1, covered_from: '', covered_to: '', overlapping_periods: false, note: 'n' }],
      ),
    );
    mount();
    const rows = await screen.findAllByTestId('budget-row');
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain('"total":"3.5"');
    expect(rows[0].textContent).toContain('usage_overlap');
    expect(rows[0].textContent).toContain('collab_relations.budget.over');
    expect(rows[1].textContent).toContain('collab_relations.budget.usage_unknown');
    expect(screen.queryByText(/4\.5/)).toBeNull();
    expect(screen.getByTestId('budget-unmatched').textContent).toContain('"unit":"EUR"');
  });

  it('renders client-written unit, source and period as inert text', async () => {
    const hostile = '<img src=x onerror=window.__pwned=1>';
    getBudget.mockResolvedValue(summary([budget({ unit: hostile, source: hostile, sampling_period: hostile })]));
    mount();
    await screen.findByTestId('budget-row');
    expect(document.querySelector('[data-testid="budget-card"] img')).toBeNull();
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });

  it('shows the server message when the budget cannot be read', async () => {
    getBudget.mockRejectedValue(new ApiError('read access denied', 403));
    mount();
    expect(await screen.findByText(/read access denied/)).toBeInTheDocument();
  });

  it('lists reported usage records on demand', async () => {
    getBudget.mockResolvedValue(summary([budget()]));
    listUsage.mockResolvedValue({
      items: [{ id: 'u1', task_id: 't1', unit: 'USD', source: 'billing-api', amount: '1.5', period_start: '2026-11-05T09:00:00Z', period_end: '2026-11-05T10:00:00Z', reported_by_kind: 'session', reported_by_id: 's1', client_usage_id: 'k', created_at: '2026-11-05T10:01:00Z', origin: 'user_content' }],
      next_cursor: null,
    });
    mount();
    await fireEvent.click(await screen.findByRole('button', { name: 'collab_relations.budget.records_show' }));
    const rec = await screen.findByTestId('usage-record');
    expect(rec.textContent).toContain('"amount":"1.5"');
    expect(rec.textContent).toContain('"kind":"session"');
  });

  it('creates a budget with expected_version 0 and replaces an existing one with its version', async () => {
    getBudget.mockResolvedValue(summary([budget({ version: 5 })]));
    setBudget.mockResolvedValue(budget());
    mount();
    await fireEvent.click(await screen.findByRole('button', { name: 'collab_relations.budget.set_open' }));
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.unit'), { target: { value: 'tokens' } });
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.source'), { target: { value: 'llm' } });
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.amount'), { target: { value: '1000' } });
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.sampling_period'), { target: { value: 'task_total' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.budget.set_submit' }));
    await waitFor(() => expect(setBudget).toHaveBeenCalledTimes(1));
    expect(setBudget.mock.calls[0][3]).toMatchObject({ expected_version: 0, unit: 'tokens', source: 'llm', amount: '1000', alert_at: null });

    // Edit the existing USD budget: the form sends the version it was built from.
    await fireEvent.click(await screen.findByRole('button', { name: 'collab_relations.budget.edit' }));
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.amount'), { target: { value: '30' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.budget.set_submit' }));
    await waitFor(() => expect(setBudget).toHaveBeenCalledTimes(2));
    expect(setBudget.mock.calls[1][3]).toMatchObject({ expected_version: 5, unit: 'USD', source: 'billing-api', amount: '30' });
  });

  it('blocks an invalid amount or token before sending anything', async () => {
    mount();
    await fireEvent.click(await screen.findByRole('button', { name: 'collab_relations.budget.set_open' }));
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.unit'), { target: { value: 'US D' } });
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.source'), { target: { value: 's' } });
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.amount'), { target: { value: '1e3' } });
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.sampling_period'), { target: { value: 'p' } });
    expect(screen.getByRole('button', { name: 'collab_relations.budget.set_submit' })).toBeDisabled();
    expect(screen.getByText(/bad_token/)).toBeInTheDocument();
    expect(screen.getByText('collab_relations.budget.bad_amount')).toBeInTheDocument();
    expect(setBudget).not.toHaveBeenCalled();
  });

  it('explains a refused budget write and reloads on VERSION_CONFLICT', async () => {
    setBudget.mockRejectedValue(new ApiError('stale', 409, 'VERSION_CONFLICT'));
    mount();
    await fireEvent.click(await screen.findByRole('button', { name: 'collab_relations.budget.set_open' }));
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.unit'), { target: { value: 'USD' } });
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.source'), { target: { value: 's' } });
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.amount'), { target: { value: '5' } });
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.sampling_period'), { target: { value: 'p' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.budget.set_submit' }));
    expect(await screen.findByText(/collab_relations\.err_budget\.VERSION_CONFLICT/)).toBeInTheDocument();
    await waitFor(() => expect(getBudget).toHaveBeenCalledTimes(2));
  });

  it('reports usage with ISO periods, and a failed attempt gets a NEW key', async () => {
    reportUsage
      .mockRejectedValueOnce(new ApiError('write access denied', 403))
      .mockResolvedValueOnce({ id: 'u1' });
    mount();
    await fireEvent.click(await screen.findByRole('button', { name: 'collab_relations.budget.usage_open' }));
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.unit'), { target: { value: 'USD' } });
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.source'), { target: { value: 'billing-api' } });
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.usage_amount'), { target: { value: '0.42' } });
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.period_start'), { target: { value: '2026-11-05T09:00' } });
    await fireEvent.input(screen.getByLabelText('collab_relations.budget.period_end'), { target: { value: '2026-11-05T10:00' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.budget.usage_submit' }));
    expect(await screen.findByText(/collab_relations\.err_forbidden/)).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.budget.usage_submit' }));
    await waitFor(() => expect(reportUsage).toHaveBeenCalledTimes(2));
    const first = reportUsage.mock.calls[0][3];
    const second = reportUsage.mock.calls[1][3];
    expect(second.client_usage_id).not.toBe(first.client_usage_id);
    expect(first).toMatchObject({ unit: 'USD', source: 'billing-api', amount: '0.42' });
    expect(first.period_start).toMatch(/^\d{4}-\d\d-\d\dT.*Z$/);
    expect(new Date(first.period_end).getTime()).toBeGreaterThan(new Date(first.period_start).getTime());
    expect(toastSuccess).toHaveBeenCalledWith('collab_relations.budget.usage_done');
  });
});
