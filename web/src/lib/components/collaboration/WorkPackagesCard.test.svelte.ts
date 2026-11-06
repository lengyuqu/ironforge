import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@testing-library/svelte';
import { ApiError } from '$lib/api/_base.svelte';

const listWorkPackages = vi.fn();
const getWorkPackage = vi.fn();

vi.mock('$lib/api/collabDecisions', () => ({
  collabDecisions: {
    listWorkPackages: (...a: unknown[]) => listWorkPackages(...a),
    getWorkPackage: (...a: unknown[]) => getWorkPackage(...a),
  },
}));
vi.mock('$lib/i18n', () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === 'object' ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

import WorkPackagesCard from './WorkPackagesCard.svelte';
import type { WorkPackage } from '$lib/api/collabDecisions';

function pkg(over: Partial<WorkPackage> = {}): WorkPackage {
  return {
    id: 'p1', task_id: 't1', revision: 2, author_kind: 'session', author_id: 'abcdef123456',
    task_version: 5, commit_sha: 'a'.repeat(40),
    goal: 'Ship it', scope: 'api only', artifacts: ['a.rs'], completed: ['tests pass'], open_items: [], next_steps: ['deploy'],
    evidence_refs: [{ kind: 'pipeline', reference: '7' }, { kind: 'note', reference: 'trust me' }],
    missing_fields: [], client_package_id: 'c', created_at: '2026-11-04T10:00:00Z',
    evidence_resolution: null, origin: 'user_content', ...over,
  };
}

function mount() {
  render(WorkPackagesCard, { props: { owner: 'alice', repo: 'demo', taskId: 't1' } });
}

describe('WorkPackagesCard.svelte', () => {
  beforeEach(() => {
    listWorkPackages.mockReset().mockResolvedValue({ items: [pkg()], next_cursor: null });
    getWorkPackage.mockReset().mockResolvedValue(
      pkg({
        evidence_resolution: [
          { verification: 'verified_reference', pipeline_id: 7, status: 'failed', commit_sha: 'b'.repeat(40), ref_name: 'main', finished_at: '2026-11-04T11:00:00Z', resolved_at: '2026-11-04T12:00:00Z' },
        ],
      }),
    );
  });

  it('lists revisions and shows nothing about verification until opened', async () => {
    mount();
    expect(await screen.findByText(/collab_decisions.wp.revision/)).toBeInTheDocument();
    expect(screen.getByText('Ship it')).toBeInTheDocument();
    expect(screen.queryByTestId('wp-verified')).toBeNull();
  });

  it('keeps the declared claims and the platform lookup in separate blocks, never merged', async () => {
    mount();
    await fireEvent.click(await screen.findByRole('button', { name: 'collab.show' }));
    const declared = await screen.findByTestId('wp-declared');
    const verified = await screen.findByTestId('wp-verified');
    expect(declared).toHaveTextContent('tests pass');
    expect(declared).toHaveTextContent('trust me');
    // The pipeline reference is only a claim here; its status lives only in the verified block.
    expect(within(declared).queryByText('collab_decisions.wp.pipeline_status.failed')).toBeNull();
    expect(within(verified).getByText('collab_decisions.wp.pipeline_status.failed')).toBeInTheDocument();
    expect(within(verified).queryByText('tests pass')).toBeNull();
    expect(declared.contains(verified)).toBe(false);
    expect(screen.getByText('collab_decisions.wp.declared_note')).toBeInTheDocument();
    expect(screen.getByText('collab_decisions.wp.verified_note')).toBeInTheDocument();
  });

  it('points out when the pipeline ran on a different commit than the one declared', async () => {
    mount();
    await fireEvent.click(await screen.findByRole('button', { name: 'collab.show' }));
    expect(await screen.findByText(/collab_decisions.wp.commit_differs/)).toBeInTheDocument();
  });

  it('reports an unavailable pipeline as not found, not as a failure of the package', async () => {
    getWorkPackage.mockResolvedValue(
      pkg({ evidence_resolution: [{ verification: 'unavailable', pipeline_id: 7, status: null, commit_sha: null, ref_name: null, finished_at: null, resolved_at: '2026-11-04T12:00:00Z' }] }),
    );
    mount();
    await fireEvent.click(await screen.findByRole('button', { name: 'collab.show' }));
    expect(await screen.findByText('collab_decisions.wp.unavailable_note')).toBeInTheDocument();
    expect(screen.queryByText(/commit_differs/)).toBeNull();
  });

  it('says nothing was verified when no pipeline was referenced', async () => {
    getWorkPackage.mockResolvedValue(pkg({ evidence_refs: [{ kind: 'note', reference: 'n' }], evidence_resolution: [] }));
    mount();
    await fireEvent.click(await screen.findByRole('button', { name: 'collab.show' }));
    expect(await screen.findByText('collab_decisions.wp.no_pipeline_refs')).toBeInTheDocument();
  });

  it('renders hostile package text as inert text', async () => {
    const hostile = '<img src=x onerror="window.__pwnedW=1"><script>window.__pwnedW=1</script>**x**';
    listWorkPackages.mockResolvedValue({ items: [pkg({ goal: hostile })], next_cursor: null });
    getWorkPackage.mockResolvedValue(pkg({ goal: hostile, completed: [hostile], evidence_refs: [{ kind: 'url', reference: hostile }], evidence_resolution: [] }));
    mount();
    await fireEvent.click(await screen.findByRole('button', { name: 'collab.show' }));
    await screen.findByTestId('wp-declared');
    const root = document.querySelector('[data-testid="work-packages"]')!;
    expect(root.querySelector('img')).toBeNull();
    expect(root.querySelector('script')).toBeNull();
    expect(root.querySelector('b, strong strong')).toBeNull();
    expect((window as unknown as { __pwnedW?: number }).__pwnedW).toBeUndefined();
  });

  it('shows the server error for a failed list and lets the user retry', async () => {
    listWorkPackages.mockRejectedValueOnce(new ApiError('read access denied', 403)).mockResolvedValue({ items: [pkg()], next_cursor: null });
    mount();
    expect(await screen.findByRole('alert')).toHaveTextContent('read access denied');
    await fireEvent.click(screen.getByRole('button', { name: 'common.retry' }));
    expect(await screen.findByText('Ship it')).toBeInTheDocument();
  });

  it('shows the error of a failed detail read without losing the list', async () => {
    getWorkPackage.mockRejectedValue(new ApiError('not found', 404));
    mount();
    await fireEvent.click(await screen.findByRole('button', { name: 'collab.show' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('not found');
    expect(screen.getByText('Ship it')).toBeInTheDocument();
  });

  it('shows an empty state and the fields the author left out', async () => {
    listWorkPackages.mockResolvedValue({ items: [pkg({ missing_fields: ['goal', 'commit_sha'], goal: '' })], next_cursor: 'c' });
    mount();
    expect(await screen.findByText(/collab_decisions.wp.left_out/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'collab.load_more' })).toBeInTheDocument();
  });
});
