import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const deleteUser = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  admin: { deleteUser: (...a: unknown[]) => deleteUser(...a) },
}));

import UserDeleteModal from './UserDeleteModal.svelte';
import type { AdminUser } from '$lib/api/client.svelte';

const user = {
  id: 5,
  username: 'bob',
  email: 'bob@example.com',
  is_admin: false,
  is_active: true,
} as unknown as AdminUser;

describe('UserDeleteModal.svelte', () => {
  beforeEach(() => {
    deleteUser.mockReset();
  });

  it('renders the confirmation dialog for the target user', () => {
    render(UserDeleteModal, { user, onClose: () => {}, onDeleted: () => {} });

    expect(screen.getByText('admin.users.delete_confirm')).toBeInTheDocument();
    expect(screen.getByText('admin.users.delete_warning')).toBeInTheDocument();
    expect(screen.getByText('common.delete')).toBeInTheDocument();
    expect(screen.getByText('common.cancel')).toBeInTheDocument();
  });

  it('deletes, closes and refreshes on confirm', async () => {
    deleteUser.mockResolvedValue(undefined);
    const onClose = vi.fn();
    const onDeleted = vi.fn().mockResolvedValue(undefined);
    render(UserDeleteModal, { user, onClose, onDeleted });

    await fireEvent.click(screen.getByText('common.delete'));
    await waitFor(() => expect(deleteUser).toHaveBeenCalledWith(5));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(onDeleted).toHaveBeenCalledTimes(1));
  });

  it('shows the inline error and stays open on failure', async () => {
    deleteUser.mockRejectedValue(new Error('cannot delete admin'));
    const onClose = vi.fn();
    render(UserDeleteModal, { user, onClose, onDeleted: () => {} });

    await fireEvent.click(screen.getByText('common.delete'));
    expect(await screen.findByText('cannot delete admin')).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });
});
