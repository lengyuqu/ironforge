import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const authLogin = vi.fn();
const authMe = vi.fn();
const authLogout = vi.fn();

vi.mock('$lib/api/client.svelte', () => ({
  auth: {
    login: (...a: unknown[]) => authLogin(...a),
    me: (...a: unknown[]) => authMe(...a),
    logout: (...a: unknown[]) => authLogout(...a),
  },
}));

vi.mock('$app/navigation', () => ({ goto: (...a: unknown[]) => goto(...a) }));
const goto = vi.fn();

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
  locale: { value: 'en', set: vi.fn(), init: () => {} },
}));

// The real auth store is used; its state is driven through the mocked
// API layer via login()/logout().
import { login, logout } from '$lib/stores/auth.svelte';

import Navbar from './Navbar.svelte';

describe('Navbar.svelte', () => {
  beforeEach(async () => {
    goto.mockClear();
    authLogin.mockReset().mockResolvedValue({ mfa_required: false });
    authMe.mockReset().mockResolvedValue({
      id: 1,
      username: 'alice',
      email: 'a@x.io',
      is_admin: false,
    });
    authLogout.mockReset().mockResolvedValue(undefined);
    await logout();
    // Drop the call made while resetting state — tests count their own.
    authLogout.mockClear();
    goto.mockClear();
  });

  it('shows sign-in / sign-up links when logged out', () => {
    render(Navbar);

    expect(screen.getByText('nav.sign_in')).toHaveAttribute('href', '/login');
    expect(screen.getByText('nav.sign_up')).toHaveAttribute('href', '/register');
    expect(screen.queryByText('nav.notifications')).not.toBeInTheDocument();
  });

  it('shows the user area after login with the avatar initial', async () => {
    render(Navbar);

    await login('alice', 'pw');
    await waitFor(() => expect(screen.getByText('alice')).toBeInTheDocument());

    expect(screen.getByText('nav.notifications')).toHaveAttribute('href', '/notifications');
    expect(screen.getByText('nav.organizations')).toHaveAttribute('href', '/orgs');
    expect(document.querySelector('.avatar')?.textContent?.trim()).toBe('A');
    // No admin link for regular users.
    expect(screen.queryByText('nav.admin_panel')).not.toBeInTheDocument();
  });

  it('routes searches, keeping the query when present', async () => {
    render(Navbar);

    const input = screen.getByPlaceholderText('Search or jump to...');
    await fireEvent.input(input, { target: { value: 'repo:alice/demo bug' } });
    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(goto).toHaveBeenCalledWith('/search?q=repo%3Aalice%2Fdemo%20bug');

    goto.mockClear();
    await fireEvent.input(input, { target: { value: '' } });
    fireEvent.click(screen.getByText('Go'));
    expect(goto).toHaveBeenCalledWith('/search');
  });

  it('signs out through the user menu and navigates to /login', async () => {
    await login('alice', 'pw');
    render(Navbar);
    await waitFor(() => expect(screen.getByText('alice')).toBeInTheDocument());

    // Open the user dropdown and sign out.
    fireEvent.click(screen.getByLabelText('User menu'));
    fireEvent.click(screen.getByText('nav.sign_out'));

    await waitFor(() => expect(authLogout).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(goto).toHaveBeenCalledWith('/login'));
    await waitFor(() => expect(screen.getByText('nav.sign_in')).toBeInTheDocument());
  });
});
