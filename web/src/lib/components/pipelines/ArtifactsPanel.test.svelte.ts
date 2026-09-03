import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const listByPipeline = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  artifacts: {
    listByPipeline: (owner: string, repo: string, pipelineId: number) =>
      listByPipeline(owner, repo, pipelineId),
  },
}));

const downloadApiFile = vi.fn();
vi.mock('$lib/api/_base.svelte', () => ({
  downloadApiFile: (path: string, fallbackFilename: string) =>
    downloadApiFile(path, fallbackFilename),
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: (...a: unknown[]) => toastError(...a),
  },
}));

import ArtifactsPanel from './ArtifactsPanel.svelte';
import type { Artifact } from '$lib/types/entities';

function makeArtifact(overrides: Partial<Artifact> = {}): Artifact {
  return {
    id: 9,
    job_id: 100,
    name: 'dist.tar.gz',
    file_path: '/data/artifacts/9/dist.tar.gz',
    size: 2048,
    created_at: new Date(Date.now() - 3 * 3_600_000).toISOString(),
    expires_at: null,
    ...overrides,
  };
}

function renderPanel(props: Record<string, unknown> = {}) {
  return render(ArtifactsPanel, {
    owner: 'alice',
    repo: 'demo',
    pipelineId: 41,
    status: 'success',
    ...props,
  });
}

describe('ArtifactsPanel.svelte', () => {
  beforeEach(() => {
    listByPipeline.mockReset();
    listByPipeline.mockResolvedValue([]);
    downloadApiFile.mockReset();
    downloadApiFile.mockResolvedValue(undefined);
    toastSuccess.mockReset();
    toastError.mockReset();
  });

  it('loads the pipeline artifacts once on mount and renders the table', async () => {
    listByPipeline.mockResolvedValue([
      makeArtifact(),
      makeArtifact({ id: 10, name: 'coverage.xml', size: 512 }),
    ]);
    renderPanel();

    expect(await screen.findByText('dist.tar.gz')).toBeInTheDocument();
    expect(listByPipeline).toHaveBeenCalledTimes(1);
    expect(listByPipeline).toHaveBeenCalledWith('alice', 'demo', 41);

    // formatBytes: 2048 -> "2.0 KB", 512 -> "512 B"; relativeTime: 3h ago.
    expect(screen.getByText('2.0 KB')).toBeInTheDocument();
    expect(screen.getByText('512 B')).toBeInTheDocument();
    expect(screen.getAllByText('3h')).toHaveLength(2);
    // No expires_at -> em dash placeholder.
    expect(screen.getAllByText('—')).toHaveLength(2);
    expect(screen.getByText('dist.tar.gz')).toHaveAttribute(
      'title',
      '/data/artifacts/9/dist.tar.gz'
    );
  });

  it('shows the empty state when the pipeline produced no artifacts', async () => {
    renderPanel();

    expect(await screen.findByText('No artifacts for this pipeline.')).toBeInTheDocument();
    expect(document.querySelector('table')).toBeNull();
  });

  it('renders expiry badges, flagging already expired artifacts', async () => {
    listByPipeline.mockResolvedValue([
      makeArtifact({ id: 11, name: 'fresh.zip', expires_at: new Date(Date.now() + 2 * 86_400_000).toISOString() }),
      makeArtifact({ id: 12, name: 'stale.zip', expires_at: new Date(Date.now() - 86_400_000).toISOString() }),
    ]);
    renderPanel();

    expect(await screen.findByText('Expires in 2d')).not.toHaveClass('expired');
    expect(screen.getByText('Expired')).toHaveClass('expired');
  });

  it('surfaces a load failure in the error banner', async () => {
    listByPipeline.mockRejectedValue(new Error('artifact store offline'));
    renderPanel();

    expect(await screen.findByText('artifact store offline')).toHaveClass('panel-error');
  });

  it('downloads an artifact by id and toasts on success', async () => {
    listByPipeline.mockResolvedValue([makeArtifact()]);
    renderPanel();

    await fireEvent.click(await screen.findByText('Download'));

    await waitFor(() =>
      expect(downloadApiFile).toHaveBeenCalledWith('/artifacts/9', 'dist.tar.gz')
    );
    await waitFor(() => expect(toastSuccess).toHaveBeenCalledWith('Download started'));
    expect(toastError).not.toHaveBeenCalled();
  });

  it('toasts the error message when the download fails', async () => {
    listByPipeline.mockResolvedValue([makeArtifact()]);
    downloadApiFile.mockRejectedValue(new Error('403 forbidden'));
    renderPanel();

    await fireEvent.click(await screen.findByText('Download'));

    await waitFor(() => expect(toastError).toHaveBeenCalledWith('403 forbidden'));
    expect(toastSuccess).not.toHaveBeenCalled();
    // The button re-enables after the attempt settles.
    await waitFor(() => expect(screen.getByText('Download')).not.toBeDisabled());
  });

  it('does not keep reloading while the pipeline is still running', async () => {
    renderPanel({ status: 'running' });

    await waitFor(() => expect(listByPipeline).toHaveBeenCalledTimes(1));
    // Give the effect several settle cycles: the load must not re-trigger itself.
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(listByPipeline).toHaveBeenCalledTimes(1);
  });

  it('reloads once when a running pipeline reaches a terminal status', async () => {
    listByPipeline.mockResolvedValue([]);
    const { rerender } = renderPanel({ status: 'running' });

    await waitFor(() => expect(listByPipeline).toHaveBeenCalledTimes(1));

    listByPipeline.mockResolvedValue([makeArtifact()]);
    await rerender({ owner: 'alice', repo: 'demo', pipelineId: 41, status: 'success' });

    expect(await screen.findByText('dist.tar.gz')).toBeInTheDocument();
    expect(listByPipeline).toHaveBeenCalledTimes(2);
  });
});
