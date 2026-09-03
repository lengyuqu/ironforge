import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

import PipelineFlow from './PipelineFlow.svelte';
import type { PipelineDetail, PipelineJob } from '$lib/types/entities';

type FlowStage = PipelineDetail['stages'][number];

function makeJob(overrides: Partial<PipelineJob> = {}): PipelineJob {
  return {
    id: 100,
    stage_id: 1,
    name: 'build',
    status: 'success',
    started_at: '2026-09-01T10:00:00Z',
    finished_at: '2026-09-01T10:00:12Z',
    ...overrides,
  };
}

function makeStage(overrides: Partial<FlowStage> = {}): FlowStage {
  return {
    id: 1,
    pipeline_id: 41,
    name: 'build',
    stage_order: 1,
    status: 'success',
    started_at: '2026-09-01T10:00:00Z',
    finished_at: '2026-09-01T10:00:30Z',
    jobs: [makeJob()],
    ...overrides,
  };
}

function makePipeline(stages: FlowStage[]): PipelineDetail {
  return {
    id: 41,
    repo_id: 7,
    commit_sha: 'abcdef1234567890',
    ref: 'main',
    status: 'success',
    trigger_type: 'push',
    started_at: '2026-09-01T10:00:00Z',
    finished_at: '2026-09-01T10:01:00Z',
    stages,
  };
}

const noopCallbacks = {
  onOpenJobLog: () => {},
  onPlayJob: () => {},
  onApproveJob: () => {},
  onRerunJob: () => {},
};

describe('PipelineFlow.svelte', () => {
  it('renders a node per stage/job with status colours and connectors between stages', () => {
    const pipeline = makePipeline([
      makeStage({ jobs: [makeJob({ name: 'compile' })] }),
      makeStage({
        id: 2,
        name: 'test',
        status: 'running',
        finished_at: undefined,
        jobs: [makeJob({ id: 200, stage_id: 2, name: 'unit', status: 'running' })],
      }),
      makeStage({
        id: 3,
        name: 'deploy',
        status: 'skipped',
        jobs: [makeJob({ id: 300, stage_id: 3, name: 'ship', status: 'skipped' })],
      }),
    ]);
    render(PipelineFlow, { pipeline, approvedJobs: [], ...noopCallbacks });

    expect(document.querySelectorAll('.flow-stage')).toHaveLength(3);
    expect([...document.querySelectorAll('.stage-name')].map((el) => el.textContent)).toEqual([
      'build',
      'test',
      'deploy',
    ]);
    expect(screen.getByText('compile')).toHaveClass('job-name');
    expect(screen.getByText('unit')).toHaveClass('job-name');
    expect(screen.getByText('ship')).toHaveClass('job-name');

    // Connector arrows sit between stages only: 3 stages -> 2 connectors.
    expect(document.querySelectorAll('.stage-connector svg')).toHaveLength(2);

    const dots = document.querySelectorAll('.stage-dot');
    expect(dots[0]).toHaveAttribute('style', expect.stringContaining('var(--green)'));
    expect(dots[1]).toHaveAttribute('style', expect.stringContaining('var(--accent)'));
    expect(dots[2]).toHaveAttribute('style', expect.stringContaining('var(--text-muted)'));

    // Durations come from formatDuration: 12s per job, 30s for the finished stage.
    expect(document.querySelector('.job-dur')).toHaveTextContent('12s');
    expect(document.querySelector('.stage-dur')).toHaveTextContent('30s');
  });

  it('applies running/failed card styling and spins only the running icon', () => {
    const pipeline = makePipeline([
      makeStage({
        jobs: [
          makeJob({ id: 100, name: 'ok', status: 'success' }),
          makeJob({ id: 101, name: 'busy', status: 'running' }),
          makeJob({ id: 102, name: 'broken', status: 'failed', exit_code: 2 }),
        ],
      }),
    ]);
    render(PipelineFlow, { pipeline, approvedJobs: [], ...noopCallbacks });

    const cards = document.querySelectorAll('.job-card');
    expect(cards[0]).not.toHaveClass('running');
    expect(cards[1]).toHaveClass('running');
    expect(cards[2]).toHaveClass('failed');

    expect(document.querySelectorAll('.spin')).toHaveLength(1);
    expect(document.querySelector('.spin')).toHaveTextContent('⟳');
    // exit_code is only rendered when present (0 included, null/undefined not).
    expect(screen.getByText('2')).toHaveClass('exit-code');
    expect(document.querySelectorAll('.exit-code')).toHaveLength(1);
  });

  it('renders exit code 0 and the environment / if-condition markers', () => {
    const pipeline = makePipeline([
      makeStage({
        jobs: [
          makeJob({
            id: 100,
            name: 'deploy',
            status: 'skipped',
            exit_code: 0,
            environment_name: 'staging',
            if_condition: "branch == 'main'",
          }),
        ],
      }),
    ]);
    render(PipelineFlow, { pipeline, approvedJobs: [], ...noopCallbacks });

    expect(screen.getByText('0')).toHaveClass('exit-code');
    expect(screen.getByText(/staging/)).toHaveClass('environment-name');
    const condition = screen.getByText('if');
    expect(condition).toHaveClass('condition-skipped');
    expect(condition).toHaveAttribute('title', "pipeline.condition_skipped: branch == 'main'");
  });

  it('opens the job log on card click and on Enter/Space keydown', async () => {
    const onOpenJobLog = vi.fn();
    const pipeline = makePipeline([makeStage({ jobs: [makeJob({ id: 100, name: 'build' })] })]);
    render(PipelineFlow, { pipeline, approvedJobs: [], ...noopCallbacks, onOpenJobLog });

    const card = document.querySelector('.job-card') as HTMLElement;
    await fireEvent.click(card);
    await fireEvent.keyDown(card, { key: 'Enter' });
    await fireEvent.keyDown(card, { key: ' ' });
    await fireEvent.keyDown(card, { key: 'ArrowDown' });

    expect(onOpenJobLog).toHaveBeenCalledTimes(3);
    expect(onOpenJobLog).toHaveBeenCalledWith(100);
  });

  it('routes manual / failed / approval buttons without opening the log', async () => {
    const onOpenJobLog = vi.fn();
    const onPlayJob = vi.fn();
    const onRerunJob = vi.fn();
    const onApproveJob = vi.fn();
    const pipeline = makePipeline([
      makeStage({
        jobs: [
          makeJob({ id: 100, name: 'manual-job', status: 'manual' }),
          makeJob({ id: 101, name: 'failed-job', status: 'failed' }),
          makeJob({ id: 102, name: 'gate', status: 'waiting_approval' }),
        ],
      }),
    ]);
    render(PipelineFlow, {
      pipeline,
      approvedJobs: [],
      onOpenJobLog,
      onPlayJob,
      onRerunJob,
      onApproveJob,
    });

    await fireEvent.click(screen.getByText('pipeline.play_manual'));
    await fireEvent.click(screen.getByText('Rerun'));
    await fireEvent.click(screen.getByText('pipeline.approve_environment'));

    expect(onPlayJob).toHaveBeenCalledWith(100);
    expect(onRerunJob).toHaveBeenCalledWith(101);
    expect(onApproveJob).toHaveBeenCalledWith(102);
    // Buttons stopPropagation, so the parent card handler must not fire.
    expect(onOpenJobLog).not.toHaveBeenCalled();
  });

  it('disables the approval button for jobs already approved locally', async () => {
    const onApproveJob = vi.fn();
    const pipeline = makePipeline([
      makeStage({ jobs: [makeJob({ id: 102, name: 'gate', status: 'waiting_approval' })] }),
    ]);
    render(PipelineFlow, {
      pipeline,
      approvedJobs: [102],
      ...noopCallbacks,
      onApproveJob,
    });

    const button = screen.getByText('pipeline.approval_recorded');
    expect(button).toBeDisabled();
    await fireEvent.click(button);
    expect(onApproveJob).not.toHaveBeenCalled();
  });
});
