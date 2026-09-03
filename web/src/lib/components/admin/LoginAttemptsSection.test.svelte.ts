import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const listLoginAttempts = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  admin: { listLoginAttempts: (...a: unknown[]) => listLoginAttempts(...a) },
}));

const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: vi.fn(), error: (...a: unknown[]) => toastError(...a), warning: vi.fn() },
}));

import LoginAttemptsSection from './LoginAttemptsSection.svelte';
import type { LoginAttemptEntry } from '$lib/api/client.svelte';

const attempt: LoginAttemptEntry = {
  id: 1,
  username: 'bob',
  auth_provider: 'password',
  success: false,
  failure_reason: 'bad password',
  ip_address: '192.168.1.9',
  user_agent: 'curl',
  created_at: '2026-09-01T10:00:00Z',
} as unknown as LoginAttemptEntry;

describe('LoginAttemptsSection.svelte', () => {
  beforeEach(() => {
    listLoginAttempts.mockReset();
    toastError.mockClear();
  });

  it('renders attempts with status, identity, ip and pagination state', () => {
    render(LoginAttemptsSection, {
      initialAttempts: [attempt],
      initialTotal: 25,
      initialPage: 1,
    });

    expect(screen.getByText('25 matching events')).toBeInTheDocument();
    expect(screen.getByText('Failed')).toBeInTheDocument();
    expect(screen.getByText('bob')).toBeInTheDocument();
    expect(screen.getByText(/bad password/)).toBeInTheDocument();
    expect(screen.getByText('192.168.1.9')).toBeInTheDocument();
    expect(screen.getByText('Page 1 of 2')).toBeInTheDocument();
    expect(screen.getByText('Previous')).toBeDisabled();
  });

  it('shows the empty state when there are no attempts', () => {
    render(LoginAttemptsSection, {
      initialAttempts: [],
      initialTotal: 0,
      initialPage: 1,
    });

    expect(screen.getByText('No matching login attempts.')).toBeInTheDocument();
    expect(screen.queryByText('Previous')).toBeNull();
  });

  it('applies the username filter and reloads the first page', async () => {
    listLoginAttempts.mockResolvedValue({
      attempts: [attempt],
      total: 1,
      page: 1,
    });
    render(LoginAttemptsSection, {
      initialAttempts: [],
      initialTotal: 0,
      initialPage: 3,
    });

    await fireEvent.input(
      screen.getByLabelText('Filter login attempts by username'),
      { target: { value: '  bob ' } }
    );
    await fireEvent.click(screen.getByText('Apply'));

    await waitFor(() =>
      expect(listLoginAttempts).toHaveBeenCalledWith(
        expect.objectContaining({
          page: 1,
          per_page: 20,
          username: 'bob',
          success: undefined,
        })
      )
    );
  });

  it('reports load failures via toast', async () => {
    listLoginAttempts.mockRejectedValue(new Error('db down'));
    render(LoginAttemptsSection, {
      initialAttempts: [],
      initialTotal: 0,
      initialPage: 1,
    });

    await fireEvent.click(screen.getByText('Apply'));
    await waitFor(() => expect(toastError).toHaveBeenCalledWith('db down'));
  });
});
