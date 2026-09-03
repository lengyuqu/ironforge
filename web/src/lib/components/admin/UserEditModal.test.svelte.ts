import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const updateUser = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  admin: { updateUser: (...a: unknown[]) => updateUser(...a) },
}));

import UserEditModal from './UserEditModal.svelte';
import type { AdminUser } from '$lib/api/client.svelte';

const user = {
  id: 5,
  username: 'bob',
  display_name: 'Bobby',
  bio: 'hello world',
  is_admin: false,
  is_active: true,
} as unknown as AdminUser;

describe('UserEditModal.svelte', () => {
  beforeEach(() => {
    updateUser.mockReset();
  });

  it('prefills the form from the user prop', () => {
    render(UserEditModal, { user, onClose: () => {}, onSaved: () => {} });

    expect(
      (screen.getByLabelText('Display Name') as HTMLInputElement).value
    ).toBe('Bobby');
    expect((screen.getByLabelText('Bio') as HTMLTextAreaElement).value).toBe('hello world');
    const checkboxes = document.querySelectorAll<HTMLInputElement>('input[type="checkbox"]');
    expect(checkboxes[0].checked).toBe(false); // is_admin
    expect(checkboxes[1].checked).toBe(true); // is_active
  });

  it('saves the edited profile and flags, then closes and refreshes', async () => {
    updateUser.mockResolvedValue(undefined);
    const onClose = vi.fn();
    const onSaved = vi.fn().mockResolvedValue(undefined);
    render(UserEditModal, { user, onClose, onSaved });

    await fireEvent.input(screen.getByLabelText('Display Name'), {
      target: { value: 'Bobby Tables' },
    });
    await fireEvent.input(screen.getByLabelText('Bio'), { target: { value: '' } });
    const checkboxes = document.querySelectorAll<HTMLInputElement>('input[type="checkbox"]');
    await fireEvent.click(checkboxes[0]); // promote to admin

    await fireEvent.click(screen.getByText('common.save'));
    await waitFor(() =>
      expect(updateUser).toHaveBeenCalledWith(5, {
        display_name: 'Bobby Tables',
        bio: undefined,
        is_admin: true,
        is_active: true,
      })
    );
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
  });

  it('shows the inline error and stays open on failure', async () => {
    updateUser.mockRejectedValue(new Error('validation failed'));
    const onClose = vi.fn();
    render(UserEditModal, { user, onClose, onSaved: () => {} });

    await fireEvent.click(screen.getByText('common.save'));
    expect(await screen.findByText('validation failed')).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });
});
