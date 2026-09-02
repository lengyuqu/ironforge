import { request, qs, type PaginatedResponse } from './_base.svelte';

/** rg-http NotificationResponse（notification::Model 的 JSON 投影） */
export interface NotificationItem {
  id: number;
  user_id: number;
  /** push / ci_triggered / issue / pr / review / pipeline ... */
  event_type: string;
  title: string;
  body: string | null;
  repo_id: number | null;
  is_read: boolean;
  created_at: string;
}

export const notifications = {
  list: (userId?: number, unreadOnly?: boolean, page?: number, perPage?: number) =>
    request<PaginatedResponse<NotificationItem>>(`/notifications${qs({ user_id: userId, unread_only: unreadOnly, page, per_page: perPage })}`),
  unreadCount: (userId?: number) =>
    request<{ unread_count: number }>(`/notifications/unread-count${userId ? `?user_id=${userId}` : ''}`),
  markRead: (id: number) =>
    request<{ id: number; is_read: boolean }>(`/notifications/${id}/read`, { method: 'POST' }),
  markAllRead: (userId?: number) =>
    request<{ marked_read: number }>(`/notifications/mark-all-read${userId ? `?user_id=${userId}` : ''}`, { method: 'POST' }),
  delete: (id: number) =>
    request<{ deleted: boolean }>(`/notifications/${id}`, { method: 'DELETE' }),
};
