import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

import PipelineList from './PipelineList.svelte';
import type { Pipeline } from '$lib/types/entities';

function makePipeline(overrides: Partial<Pipeline> = {}): Pipeline {
  return {
    id: 41,
    repo_id: 7,
    commit_sha: 'abcdef1234567890',
    ref_name: 'main',
    status: 'success',
    trigger_type: 'push',
    started_at: '2026-09-01T10:00:00Z',
    finished_at: '2026-09-01T10:01:30Z',
    ...overrides,
  };
}

describe('PipelineList.svelte', () => {
  it('renders one row per pipeline with short sha, ref and duration', () => {
    render(PipelineList, {
      pipelines: [makePipeline(), makePipeline({ id: 42, ref_name: 'dev', status: 'running' })],
      onSelect: () => {},
    });

    expect(screen.getByText('repo.tabs.pipelines')).toBeInTheDocument();
    expect(screen.getByText('#41 · main')).toBeInTheDocument();
    expect(screen.getByText('#42 · dev')).toBeInTheDocument();
    // commit_sha is truncated to 7 chars, both rows share the same sha.
    expect(screen.getAllByText('abcdef1')).toHaveLength(2);
    // 90s between started_at and finished_at.
    expect(screen.getAllByText('1m 30s')).toHaveLength(2);
    // Status badges come from PipelineBadge (key passthrough via the t() mock).
    expect(screen.getByText(/pipeline\.status\.success/)).toBeInTheDocument();
    expect(screen.getByText(/pipeline\.status\.running/)).toBeInTheDocument();
  });

  it('marks only the selected pipeline row active', () => {
    render(PipelineList, {
      pipelines: [makePipeline(), makePipeline({ id: 42, ref_name: 'dev' })],
      selectedId: 42,
      onSelect: () => {},
    });

    const rows = document.querySelectorAll('.pipeline-item');
    expect(rows).toHaveLength(2);
    expect(rows[0]).not.toHaveClass('active');
    expect(rows[1]).toHaveClass('active');
  });

  it('reports the clicked pipeline id through onSelect', async () => {
    const onSelect = vi.fn();
    render(PipelineList, {
      pipelines: [makePipeline(), makePipeline({ id: 42, ref_name: 'dev' })],
      onSelect,
    });

    await fireEvent.click(screen.getByText('#42 · dev'));
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledWith(42);
  });

  it('selects by keyboard on Enter and Space but ignores other keys', async () => {
    const onSelect = vi.fn();
    render(PipelineList, { pipelines: [makePipeline()], onSelect });

    const row = document.querySelector('.pipeline-item') as HTMLElement;
    await fireEvent.keyDown(row, { key: 'Enter' });
    await fireEvent.keyDown(row, { key: ' ' });
    await fireEvent.keyDown(row, { key: 'Tab' });

    expect(onSelect).toHaveBeenCalledTimes(2);
    expect(onSelect).toHaveBeenNthCalledWith(1, 41);
    expect(onSelect).toHaveBeenNthCalledWith(2, 41);
  });

  it('renders an empty scroll area when there are no pipelines', () => {
    render(PipelineList, { pipelines: [], onSelect: () => {} });

    expect(document.querySelectorAll('.pipeline-item')).toHaveLength(0);
    expect(document.querySelector('.list-scroll')).toBeInTheDocument();
  });
});
