import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const markRead = vi.fn();

vi.mock('$lib/api/client.svelte', () => ({
  notifications: { markRead: (...a: unknown[]) => markRead(...a) },
}));

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
  formatDateTime: (iso: string) => `fmt(${iso})`,
}));

import NotificationList from './NotificationList.svelte';
import type { NotificationItem } from '$lib/api/client.svelte';

function makeItem(overrides: Partial<NotificationItem> = {}): NotificationItem {
  return {
    id: 1,
    user_id: 10,
    event_type: 'issue',
    title: 'Issue assigned to you',
    body: 'Please take a look',
    repo_id: 1,
    is_read: false,
    created_at: '2026-09-01T00:00:00Z',
    ...overrides,
  };
}

function renderList(overrides: Record<string, unknown> = {}) {
  const onRefresh = vi.fn().mockResolvedValue(undefined);
  const onError = vi.fn();
  const props = {
    items: [] as NotificationItem[],
    loading: false,
    onRefresh,
    onError,
    ...overrides,
  };
  render(NotificationList, props);
  return { onRefresh, onError };
}

describe('NotificationList.svelte', () => {
  it('shows a loading placeholder while loading', () => {
    renderList({ loading: true });

    expect(screen.getByText('common.loading')).toBeInTheDocument();
    expect(document.querySelector('.notif-list')).not.toBeInTheDocument();
  });

  it('shows the empty state when there are no notifications', () => {
    renderList();

    expect(screen.getByText('notifications.empty')).toBeInTheDocument();
  });

  it('renders items with icons, bodies and formatted timestamps', () => {
    renderList({
      items: [
        makeItem(),
        makeItem({ id: 2, event_type: 'push', title: 'Push received', body: null, is_read: true }),
      ],
    });

    expect(document.querySelector('.notif-icon')?.textContent).toBe('❗');
    expect(screen.getByText('Issue assigned to you')).toBeInTheDocument();
    expect(screen.getByText('Please take a look')).toBeInTheDocument();
    // Both items share a timestamp — assert on all matches.
    expect(screen.getAllByText('fmt(2026-09-01T00:00:00Z)').length).toBe(2);

    // Null body is omitted entirely; read items carry no unread styling.
    const items = document.querySelectorAll('.notif-item');
    expect(items[0].classList.contains('unread')).toBe(true);
    expect(items[1].classList.contains('unread')).toBe(false);
    expect(items[1].querySelector('.notif-body')).not.toBeInTheDocument();
    expect(items[1].querySelector('.notif-icon')?.textContent).toBe('📦');
  });

  it('marks unread notifications read via the API and refreshes', async () => {
    markRead.mockReset().mockResolvedValue(undefined);
    const { onRefresh } = renderList({ items: [makeItem()] });

    await fireEvent.click(screen.getByText('notifications.mark_read'));
    await waitFor(() => expect(markRead).toHaveBeenCalledWith(1));
    await waitFor(() => expect(onRefresh).toHaveBeenCalledTimes(1));
  });

  it('routes mark-read failures to onError with the error message', async () => {
    markRead.mockReset().mockRejectedValue(new Error('boom'));
    const { onError } = renderList({ items: [makeItem()] });

    await fireEvent.click(screen.getByText('notifications.mark_read'));
    await waitFor(() => expect(onError).toHaveBeenCalledWith('boom'));
  });
});
