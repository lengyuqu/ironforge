import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import OrgList from './OrgList.svelte';
import type { Organization } from '$lib/types/entities';

const orgs: Organization[] = [
  {
    id: 1,
    name: 'acme',
    display_name: 'Acme Inc',
    description: 'Making everything',
    visibility: 'public',
    created_at: '2026-01-01T00:00:00Z',
  } as unknown as Organization,
  {
    id: 2,
    name: 'secret',
    display_name: null,
    description: null,
    visibility: 'private',
    created_at: '2026-02-01T00:00:00Z',
  } as unknown as Organization,
];

describe('OrgList.svelte', () => {
  it('renders org cards with links, visibility and fallbacks', () => {
    render(OrgList, { organizations: orgs, loading: false, onCreate: () => {} });

    expect(screen.getByText('Acme Inc').closest('a')).toHaveAttribute('href', '/orgs/acme');
    expect(screen.getByText('@acme')).toBeInTheDocument();
    expect(screen.getByText('Making everything')).toBeInTheDocument();
    expect(screen.getByText('orgs.visibility_public')).toBeInTheDocument();

    // Second org falls back to the slug name and the no-description key.
    expect(screen.getByText('secret').closest('a')).toHaveAttribute('href', '/orgs/secret');
    expect(screen.getByText('orgs.visibility_private')).toBeInTheDocument();
    expect(screen.getByText('common.no_description')).toBeInTheDocument();
    expect(screen.getByText('A')).toBeInTheDocument(); // avatar initial
  });

  it('shows the empty state with a create action', async () => {
    const onCreate = vi.fn();
    render(OrgList, { organizations: [], loading: false, onCreate });

    expect(screen.getByText('orgs.no_orgs')).toBeInTheDocument();
    await fireEvent.click(screen.getByText('orgs.create_action'));
    expect(onCreate).toHaveBeenCalledTimes(1);
  });

  it('shows the loading state instead of the list', () => {
    render(OrgList, { organizations: orgs, loading: true, onCreate: () => {} });

    expect(screen.getByText('common.loading')).toBeInTheDocument();
    expect(screen.queryByText('@acme')).toBeNull();
  });
});
