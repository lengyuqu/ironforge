import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({ createT: () => (key: string) => `T(${key})` }));

import StatusBadge from './StatusBadge.svelte';

describe('StatusBadge.svelte', () => {
  it('translates known values', () => {
    render(StatusBadge, { value: 'blocked', prefix: 'collab.task_status', known: ['open', 'blocked'] });
    expect(screen.getByText('T(collab.task_status.blocked)')).toBeInTheDocument();
  });

  it('shows an unknown value from a newer server as inert raw text', () => {
    render(StatusBadge, { value: '<b>weird</b>', prefix: 'collab.task_status', known: ['open'] });
    const el = screen.getByText('<b>weird</b>');
    expect(el.querySelector('b')).toBeNull();
  });

  it('renders a dash for a missing value', () => {
    render(StatusBadge, { value: null, prefix: 'p', known: [] });
    expect(screen.getByText('—')).toBeInTheDocument();
  });
});
