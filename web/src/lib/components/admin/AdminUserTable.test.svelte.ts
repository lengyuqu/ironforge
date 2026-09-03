import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

const unlockUser = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  admin: { unlockUser: (...a: unknown[]) => unlockUser(...a) },
}));

vi.mock('$lib/stores/auth.svelte', () => ({
  getUser: () => ({ id: 1 }),
}));

vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));

import AdminUserTable from './AdminUserTable.svelte';
import type { AdminUser } from '$lib/api/client.svelte';

const baseUser: AdminUser = {
  id: 2,
  username: 'bob',
  email: 'bob@example.com',
  is_admin: true,
  is_active: true,
  auth_provider: 'password',
  login_attempts: 0,
  locked_until: null,
  last_login_at: '2026-09-01T08:00:00Z',
  created_at: '2026-01-01T00:00:00Z',
} as unknown as AdminUser;

function makeUser(overrides: Partial<AdminUser>): AdminUser {
  return { ...baseUser, ...overrides } as AdminUser;
}

describe('AdminUserTable.svelte', () => {
  beforeEach(() => {
    unlockUser.mockReset().mockResolvedValue(undefined);
  });

  it('renders rows with badges, login state and pagination info', () => {
    render(AdminUserTable, {
      users: [
        makeUser({}),
        makeUser({
          id: 3,
          username: 'carol',
          email: 'carol@example.com',
          is_admin: false,
          is_active: false,
          login_attempts: 2,
        }),
      ],
      page: 1,
      totalPages: 2,
      onPageChange: () => {},
      onEdit: () => {},
      onDelete: () => {},
      onRefresh: () => {},
    });

    expect(screen.getByText('bob')).toBeInTheDocument();
    expect(screen.getByText('carol')).toBeInTheDocument();
    expect(screen.getByText('bob@example.com')).toBeInTheDocument();
    // carol is inactive and has failed attempts -> warning badge, unlock button
    expect(screen.getByText('2 failed')).toBeInTheDocument();
    expect(screen.getByText('Unlock')).toBeInTheDocument();
    // locked user in the next case uses title; here carol shows failed badge
    expect(screen.getByText('Page 1 of 2')).toBeInTheDocument();
    expect(screen.getByText('← Prev')).toBeDisabled();
  });

  it('marks locked users and hides delete for the current admin row', () => {
    const future = new Date(Date.now() + 60_000).toISOString();
    render(AdminUserTable, {
      users: [
        makeUser({ locked_until: future }), // bob (id 2), not current user
        makeUser({ id: 1, username: 'admin', locked_until: null }), // current user row
      ],
      page: 1,
      totalPages: 1,
      onPageChange: () => {},
      onEdit: () => {},
      onDelete: () => {},
      onRefresh: () => {},
    });

    expect(screen.getByText('Locked')).toBeInTheDocument();
    // Delete rendered for bob but not for the admin's own row (id 1).
    expect(screen.getAllByText('common.delete').length).toBe(1);
  });

  it('unlock flow calls the API then refreshes the list', async () => {
    const onRefresh = vi.fn().mockResolvedValue(undefined);
    const future = new Date(Date.now() + 60_000).toISOString();
    render(AdminUserTable, {
      users: [makeUser({ locked_until: future })],
      page: 1,
      totalPages: 1,
      onPageChange: () => {},
      onEdit: () => {},
      onDelete: () => {},
      onRefresh,
    });

    await fireEvent.click(screen.getByText('Unlock'));
    await waitFor(() => expect(unlockUser).toHaveBeenCalledWith(2));
    await waitFor(() => expect(onRefresh).toHaveBeenCalledTimes(1));
  });
});
