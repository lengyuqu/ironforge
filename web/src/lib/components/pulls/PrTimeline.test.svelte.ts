import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import PrTimeline from './PrTimeline.svelte';
import type { PrTimelineEvent } from '$lib/types/entities';

function makeEvent(overrides: Partial<PrTimelineEvent> = {}): PrTimelineEvent {
  return {
    id: '1',
    kind: 'opened',
    actor: { id: 1, username: 'bob' },
    created_at: '2026-03-04T00:00:00Z',
    body: null,
    metadata: {},
    ...overrides,
  };
}

describe('PrTimeline.svelte', () => {
  it('renders the timeline title even with no events', () => {
    // `events` collides with Svelte's mount option, so props must be wrapped.
    render(PrTimeline, { props: { events: [] } });
    expect(screen.getByText('pulls.timeline.title')).toBeInTheDocument();
  });

  it('renders each event with actor, kind label and formatted date', () => {
    render(PrTimeline, { props: { events: [makeEvent()] } });
    expect(screen.getByText('bob')).toBeInTheDocument();
    expect(screen.getByText('pulls.timeline.opened')).toBeInTheDocument();
    expect(screen.getByText('fmt(2026-03-04T00:00:00Z)')).toBeInTheDocument();
  });

  it('falls back to the system actor label when there is no actor', () => {
    render(PrTimeline, { props: { events: [makeEvent({ actor: null })] } });
    expect(screen.getByText('pulls.timeline.system')).toBeInTheDocument();
  });

  it('shows the path with a single line from metadata', () => {
    render(PrTimeline, {
      props: { events: [makeEvent({ kind: 'comment', metadata: { path: 'src/a.ts', line: 10 } })] },
    });
    expect(screen.getByText('src/a.ts:10')).toBeInTheDocument();
  });

  it('renders a multi-line range when start_line differs from line', () => {
    render(PrTimeline, {
      props: {
        events: [makeEvent({ kind: 'comment', metadata: { path: 'src/a.ts', start_line: 8, line: 10 } })],
      },
    });
    expect(screen.getByText('src/a.ts:8-10')).toBeInTheDocument();
  });

  it('renders the body when present', () => {
    render(PrTimeline, { props: { events: [makeEvent({ body: 'left a note' })] } });
    expect(screen.getByText('left a note')).toBeInTheDocument();
  });
});
