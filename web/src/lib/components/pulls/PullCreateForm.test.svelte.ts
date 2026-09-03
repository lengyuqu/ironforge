import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const pullsCreate = vi.fn();
const pullsTemplate = vi.fn();
const reposBranches = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  pulls: {
    create: (...a: unknown[]) => pullsCreate(...a),
    template: (...a: unknown[]) => pullsTemplate(...a),
  },
  repos: {
    branches: (...a: unknown[]) => reposBranches(...a),
  },
}));

const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: vi.fn(), error: (...a: unknown[]) => toastError(...a) },
}));

import PullCreateForm from './PullCreateForm.svelte';

// `feature` is listed first so happy-dom (which cannot honour an explicit
// <select> value) coerces the bound head branch to the first enabled option.
const BRANCHES = [
  { name: 'feature', is_default: false },
  { name: 'main', is_default: true },
];

beforeEach(() => {
  [pullsCreate, pullsTemplate, reposBranches].forEach((f) => f.mockReset());
  toastError.mockClear();
  pullsCreate.mockResolvedValue({});
  pullsTemplate.mockResolvedValue(undefined);
  reposBranches.mockResolvedValue(BRANCHES);
});

function titleInput(container: HTMLElement): HTMLInputElement {
  return container.querySelector('input[type="text"]') as HTMLInputElement;
}
function headSelect(container: HTMLElement): HTMLSelectElement {
  return container.querySelectorAll('select')[0] as HTMLSelectElement;
}

describe('PullCreateForm.svelte', () => {
  it('renders the form with from/into branch selectors', async () => {
    render(PullCreateForm, { owner: 'o', repo: 'r', onCreated: () => {}, onCancel: () => {} });
    expect(screen.getByText('pulls.create_form.title')).toBeInTheDocument();
    expect(screen.getByText('pulls.create_form.from')).toBeInTheDocument();
    // both selects list the same two branches
    expect(await screen.findAllByText('feature')).toHaveLength(2);
    expect(screen.getByText('main')).toBeInTheDocument();
  });

  it('disables submit until a title and head branch are provided', async () => {
    render(PullCreateForm, { owner: 'o', repo: 'r', onCreated: () => {}, onCancel: () => {} });
    await screen.findAllByText('feature');
    expect((screen.getByText('pulls.create_form.submit') as HTMLButtonElement).disabled).toBe(true);
  });

  it('shows a required-title error for an empty title', async () => {
    render(PullCreateForm, { owner: 'o', repo: 'r', onCreated: () => {}, onCancel: () => {} });
    expect(await screen.findByText('pulls.create_form.title_required')).toBeInTheDocument();
  });

  it('creates a PR with the chosen head branch, title and default base', async () => {
    const onCreated = vi.fn();
    const { container } = render(PullCreateForm, { owner: 'o', repo: 'r', onCreated, onCancel: () => {} });
    await screen.findAllByText('feature');
    // happy-dom coerces the select to its first enabled option ("feature")
    await fireEvent.change(headSelect(container));
    await fireEvent.input(titleInput(container), { target: { value: 'My new PR' } });

    expect((screen.getByText('pulls.create_form.submit') as HTMLButtonElement).disabled).toBe(false);
    await fireEvent.click(screen.getByText('pulls.create_form.submit'));

    await waitFor(() =>
      expect(pullsCreate).toHaveBeenCalledWith('o', 'r', {
        title: 'My new PR',
        body: undefined,
        head_branch: 'feature',
        base_branch: 'main',
        draft: false,
      })
    );
    await waitFor(() => expect(onCreated).toHaveBeenCalledTimes(1));
  });

  it('creates a draft PR when the draft checkbox is set', async () => {
    const onCreated = vi.fn();
    const { container } = render(PullCreateForm, { owner: 'o', repo: 'r', onCreated, onCancel: () => {} });
    await screen.findAllByText('feature');
    await fireEvent.change(headSelect(container));
    await fireEvent.input(titleInput(container), { target: { value: 'Draft PR' } });
    await fireEvent.click(screen.getByLabelText('pulls.create_form.draft'));
    await fireEvent.click(screen.getByText('pulls.create_form.submit'));

    await waitFor(() =>
      expect(pullsCreate).toHaveBeenCalledWith('o', 'r', {
        title: 'Draft PR',
        body: undefined,
        head_branch: 'feature',
        base_branch: 'main',
        draft: true,
      })
    );
  });

  it('shows an error banner when creation fails', async () => {
    pullsCreate.mockRejectedValue(new Error('branch conflict'));
    const { container } = render(PullCreateForm, { owner: 'o', repo: 'r', onCreated: () => {}, onCancel: () => {} });
    await screen.findAllByText('feature');
    await fireEvent.change(headSelect(container));
    await fireEvent.input(titleInput(container), { target: { value: 'Broken' } });
    await fireEvent.click(screen.getByText('pulls.create_form.submit'));
    expect(await screen.findByText('branch conflict')).toBeInTheDocument();
  });

  it('calls onCancel when the cancel button is clicked', async () => {
    const onCancel = vi.fn();
    render(PullCreateForm, { owner: 'o', repo: 'r', onCreated: () => {}, onCancel });
    await fireEvent.click(screen.getByText('pulls.create_form.cancel'));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});
