// How many notifications the signed-in user has not read, for the badge in the
// navigation bar. A nudge, not a source of truth: it is refreshed on a timer
// and whenever the notifications page loads, and a failed refresh just keeps
// the last number.
import { notifications } from '$lib/api/client.svelte';

let count = $state(0);

export function getUnreadCount(): number {
  return count;
}

export function setUnreadCount(n: number): void {
  count = Number.isFinite(n) && n > 0 ? Math.floor(n) : 0;
}

export async function refreshUnreadCount(userId?: number): Promise<void> {
  try {
    const res = await notifications.unreadCount(userId);
    setUnreadCount(res.unread_count ?? 0);
  } catch {
    // Keep the previous value; the badge is only a hint.
  }
}
