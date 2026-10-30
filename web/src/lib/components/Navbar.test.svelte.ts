import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

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

  // Layout contract. happy-dom does not run layout and drops the component's
  // <style> block, so the CSS cannot be asserted through the DOM; read the
  // component source instead. These pin the phone-width rules that keep the
  // search button on one line: CJK text breaks between any two characters, so
  // a squeezed button rendered "搜索" as two stacked characters.
  describe('narrow-screen layout rules', () => {
    // `new URL(..., import.meta.url)` throws under happy-dom's URL shim, so
    // build the path from the test file's own directory instead.
    const source = readFileSync(
      resolve(import.meta.dirname ?? process.cwd(), 'Navbar.svelte'),
      'utf8'
    );
    const styleBlock = source.slice(source.indexOf('<style>'), source.lastIndexOf('</style>'));

    /** Declarations of the first rule whose selector list matches `selector`. */
    function ruleFor(selector: string, scope = styleBlock): string {
      const start = scope.indexOf(`${selector} {`);
      if (start < 0) throw new Error(`no rule for ${selector}`);
      return scope.slice(start, scope.indexOf('}', start));
    }

    /** Body of a media query, e.g. `@media (max-width: 900px)`. */
    function mediaBlock(query: string): string {
      const start = styleBlock.indexOf(`@media ${query}`);
      if (start < 0) throw new Error(`no media block for ${query}`);
      return styleBlock.slice(start, styleBlock.indexOf('\n  }\n', start));
    }

    it('keeps the search button label on one line', () => {
      const btn = ruleFor('.search-btn');
      expect(btn).toMatch(/white-space:\s*nowrap/);
      // Without this the input (flex-basis 100%) squeezes the button to its
      // min-content width, which for CJK is a single character.
      expect(btn).toMatch(/flex:\s*0\s+0\s+auto/);
    });

    it('lets the search input shrink instead of overflowing on narrow screens', () => {
      const input = ruleFor('.search-input', mediaBlock('(max-width: 900px)'));
      expect(input).toMatch(/min-width:\s*0/);
      expect(input).toMatch(/flex:\s*1\s+1\s+auto/);
      // The button must not be given up on as the screen narrows.
      expect(input).not.toMatch(/min-width:\s*180px/);
    });

    it('keeps the desktop grid untouched', () => {
      // The single-column layout only applies below 900px; the desktop rule
      // still lays the three areas out side by side.
      const inner = ruleFor('.navbar-inner');
      expect(inner).toMatch(
        /grid-template-columns:\s*minmax\(260px,\s*1\.2fr\)\s+minmax\(240px,\s*1fr\)\s+minmax\(320px,\s*1\.2fr\)/
      );
    });
  });
});
