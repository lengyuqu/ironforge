import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

type LogStatus = 'connected' | 'reconnecting' | 'closed';

/** Callbacks captured from the last connectJobLogWebSocket call. */
let captured: {
  jobId: number;
  onLog: (chunk: string) => void;
  onStatus?: (status: LogStatus) => void;
  onError?: (err: Event) => void;
  since?: number;
} | null = null;

const connectSpy = vi.fn();
const disconnectSpy = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  connectJobLogWebSocket: (
    jobId: number,
    onLog: (chunk: string) => void,
    onStatus?: (status: LogStatus) => void,
    onError?: (err: Event) => void,
    since?: number
  ) => {
    captured = { jobId, onLog, onStatus, onError, since };
    connectSpy(jobId, since);
    return null;
  },
  disconnectJobLogWebSocket: () => disconnectSpy(),
}));

import JobLogModal from './JobLogModal.svelte';
import type { PipelineJob } from '$lib/types/entities';

function makeJob(overrides: Partial<PipelineJob> = {}): PipelineJob {
  return {
    id: 100,
    stage_id: 1,
    name: 'build',
    status: 'running',
    log: 'step 1\nstep 2',
    ...overrides,
  };
}

describe('JobLogModal.svelte', () => {
  beforeEach(() => {
    captured = null;
    connectSpy.mockReset();
    disconnectSpy.mockReset();
  });

  it('renders the job header/log and opens the stream from the preloaded line count', () => {
    render(JobLogModal, { job: makeJob(), onClose: () => {} });

    expect(screen.getByText('build')).toBeInTheDocument();
    expect(screen.getByText(/pipeline\.status\.running/)).toBeInTheDocument();
    expect(document.querySelector('.log-content')).toHaveTextContent('step 1 step 2');
    // `since` resumes after the 2 lines already rendered.
    expect(connectSpy).toHaveBeenCalledWith(100, 2);
  });

  it('reflects the stream status badge and appends streamed chunks', async () => {
    render(JobLogModal, { job: makeJob({ log: '' }), onClose: () => {} });

    expect(connectSpy).toHaveBeenCalledWith(100, 0);
    // No status yet -> idle, no badge rendered.
    expect(document.querySelector('.log-live')).toBeNull();
    expect(document.querySelector('.log-content')).toHaveTextContent('(no log output)');

    captured?.onStatus?.('connected');
    expect(await screen.findByText('Live')).toHaveClass('connected');

    captured?.onLog('hello\n');
    captured?.onLog('world');
    await waitFor(() =>
      expect(document.querySelector('.log-content')).toHaveTextContent('hello world')
    );

    captured?.onStatus?.('reconnecting');
    expect(await screen.findByText('Reconnecting…')).toBeInTheDocument();

    captured?.onStatus?.('closed');
    expect(await screen.findByText('Closed')).toBeInTheDocument();
  });

  it('ignores empty chunks so the rendered log is untouched', async () => {
    render(JobLogModal, { job: makeJob({ log: 'kept' }), onClose: () => {} });

    captured?.onLog('');
    await waitFor(() => expect(document.querySelector('.log-content')).toHaveTextContent('kept'));
    expect(document.querySelector('.log-content')?.textContent).toBe('kept');
  });

  it('shows the socket error message when the connection fails', async () => {
    render(JobLogModal, { job: makeJob(), onClose: () => {} });

    captured?.onError?.(new Event('error'));
    const badge = await screen.findByText('Live log connection failed');
    expect(badge).toHaveClass('error');
  });

  it('renders initialError instead of connecting when the job fetch failed', () => {
    render(JobLogModal, { job: makeJob(), initialError: 'job 100 not found', onClose: () => {} });

    expect(screen.getByText('job 100 not found')).toHaveClass('log-live', 'error');
    expect(document.querySelector('.log-content')).toHaveTextContent('(no log output)');
    expect(connectSpy).not.toHaveBeenCalled();
  });

  it('closes via the ✕ button, the overlay and Escape, disconnecting each time', async () => {
    const onClose = vi.fn();
    render(JobLogModal, { job: makeJob(), onClose });

    await fireEvent.click(screen.getByText('✕'));
    expect(onClose).toHaveBeenCalledTimes(1);

    await fireEvent.click(document.querySelector('.log-overlay') as HTMLElement);
    expect(onClose).toHaveBeenCalledTimes(2);

    await fireEvent.keyDown(document.querySelector('.log-modal') as HTMLElement, {
      key: 'Escape',
    });
    expect(onClose).toHaveBeenCalledTimes(3);

    // Every close path tears the socket down first.
    expect(disconnectSpy).toHaveBeenCalledTimes(3);
  });

  it('closes on Enter/Space, ignores unrelated keys and disconnects on unmount', async () => {
    const onClose = vi.fn();
    const { unmount } = render(JobLogModal, { job: makeJob(), onClose });

    const dialog = document.querySelector('.log-modal') as HTMLElement;
    await fireEvent.keyDown(dialog, { key: 'a' });
    expect(onClose).not.toHaveBeenCalled();

    await fireEvent.keyDown(dialog, { key: 'Enter' });
    await fireEvent.keyDown(dialog, { key: ' ' });
    expect(onClose).toHaveBeenCalledTimes(2);

    disconnectSpy.mockClear();
    unmount();
    expect(disconnectSpy).toHaveBeenCalledTimes(1);
  });

  it('shows the if-condition, labelled as skipped when the job was skipped', () => {
    render(JobLogModal, {
      job: makeJob({ status: 'skipped', if_condition: "branch == 'main'" }),
      onClose: () => {},
    });

    expect(screen.getByText('pipeline.condition_skipped')).toBeInTheDocument();
    expect(screen.getByText("branch == 'main'").tagName).toBe('CODE');
  });

  it('falls back to a generic title and the plain condition label', () => {
    render(JobLogModal, {
      job: makeJob({ name: '', status: 'success', if_condition: 'always()' }),
      onClose: () => {},
    });

    expect(screen.getByText('Job Log')).toBeInTheDocument();
    expect(screen.getByText('pipeline.condition')).toBeInTheDocument();
  });
});
