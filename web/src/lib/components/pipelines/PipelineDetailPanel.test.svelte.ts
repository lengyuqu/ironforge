import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

// The nested ArtifactsPanel loads on mount; keep it inert.
const listByPipeline = vi.fn(() => Promise.resolve([]));
vi.mock('$lib/api/client.svelte', () => ({
  artifacts: {
    listByPipeline: (owner: string, repo: string, pipelineId: number) =>
      listByPipeline(),
  },
}));
vi.mock('$lib/api/_base.svelte', () => ({
  downloadApiFile: () => Promise.resolve(),
}));
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: () => {}, error: () => {} },
}));

import PipelineDetailPanel from './PipelineDetailPanel.svelte';
import type { PipelineDetail, PipelineJob } from '$lib/types/entities';

type FlowStage = PipelineDetail['stages'][number];

function makeJob(overrides: Partial<PipelineJob> = {}): PipelineJob {
  return {
    id: 100,
    stage_id: 1,
    name: 'compile',
    status: 'success',
    started_at: '2026-09-01T10:00:00Z',
    finished_at: '2026-09-01T10:00:12Z',
    ...overrides,
  };
}

function makePipeline(overrides: Partial<PipelineDetail> = {}): PipelineDetail {
  return {
    id: 41,
    repo_id: 7,
    commit_sha: 'abcdef1234567890',
    ref: 'main',
    status: 'success',
    trigger_type: 'push',
    started_at: '2026-09-01T10:00:00Z',
    finished_at: '2026-09-01T10:01:00Z',
    stages: [
      {
        id: 1,
        pipeline_id: 41,
        name: 'build',
        stage_order: 1,
        status: 'success',
        started_at: '2026-09-01T10:00:00Z',
        finished_at: '2026-09-01T10:00:30Z',
        jobs: [makeJob()],
      } satisfies FlowStage,
    ],
    ...overrides,
  };
}

const callbacks = {
  onRetry: () => {},
  onCancel: () => {},
  onOpenJobLog: () => {},
  onPlayJob: () => {},
  onApproveJob: () => {},
  onRerunJob: () => {},
};

function renderPanel(props: Record<string, unknown> = {}) {
  return render(PipelineDetailPanel, {
    owner: 'alice',
    repo: 'demo',
    pipeline: makePipeline(),
    approvedJobs: [],
    ...callbacks,
    ...props,
  });
}

describe('PipelineDetailPanel.svelte', () => {
  beforeEach(() => {
    listByPipeline.mockClear();
  });

  it('renders the header, short commit, ref, duration, flow and artifacts panel', () => {
    renderPanel();

    expect(screen.getByText('pipeline.detail_title')).toBeInTheDocument();
    expect(screen.getByText(/pipeline\.status\.success/)).toBeInTheDocument();
    expect(screen.getByText('abcdef1').tagName).toBe('CODE');
    expect(screen.getByText(/main/)).toBeInTheDocument();
    // 60s between started_at and finished_at.
    expect(screen.getByText(/1m 0s/)).toBeInTheDocument();
    // PipelineFlow rendered for the single stage; ArtifactsPanel always mounts.
    expect(screen.getByText('compile')).toHaveClass('job-name');
    expect(document.querySelector('.artifacts-panel')).toBeInTheDocument();
    expect(listByPipeline).toHaveBeenCalledTimes(1);
  });

  it('shows the placeholder instead of the flow when the pipeline has no stages', () => {
    renderPanel({ pipeline: makePipeline({ stages: [] }) });

    expect(screen.getByText('pipeline.select_detail')).toBeInTheDocument();
    expect(document.querySelector('.pipeline-flow')).toBeNull();
  });

  it('offers retry only for failed pipelines and delegates the pipeline id', async () => {
    const onRetry = vi.fn();
    renderPanel({ pipeline: makePipeline({ status: 'failed' }), onRetry });

    expect(screen.queryByText('pipeline.cancel')).toBeNull();
    await fireEvent.click(screen.getByText('pipeline.retry'));
    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(onRetry).toHaveBeenCalledWith(41);
  });

  it('offers cancel only for in-flight pipelines and delegates the pipeline id', async () => {
    const onCancel = vi.fn();
    renderPanel({ pipeline: makePipeline({ status: 'waiting_approval' }), onCancel });

    expect(screen.queryByText('pipeline.retry')).toBeNull();
    await fireEvent.click(screen.getByText('pipeline.cancel'));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onCancel).toHaveBeenCalledWith(41);
  });

  it('hides both actions for a successful pipeline', () => {
    renderPanel();

    expect(screen.queryByText('pipeline.retry')).toBeNull();
    expect(screen.queryByText('pipeline.cancel')).toBeNull();
  });

  it('forwards job callbacks from the nested flow', async () => {
    const onOpenJobLog = vi.fn();
    const onRerunJob = vi.fn();
    renderPanel({
      pipeline: makePipeline({
        status: 'failed',
        stages: [
          {
            id: 1,
            pipeline_id: 41,
            name: 'build',
            stage_order: 1,
            status: 'failed',
            started_at: '2026-09-01T10:00:00Z',
            finished_at: '2026-09-01T10:00:30Z',
            jobs: [makeJob({ id: 101, name: 'compile', status: 'failed' })],
          } satisfies FlowStage,
        ],
      }),
      onOpenJobLog,
      onRerunJob,
    });

    await fireEvent.click(screen.getByText('Rerun'));
    expect(onRerunJob).toHaveBeenCalledWith(101);
    expect(onOpenJobLog).not.toHaveBeenCalled();

    await fireEvent.click(document.querySelector('.job-card') as HTMLElement);
    expect(onOpenJobLog).toHaveBeenCalledWith(101);
  });
});
