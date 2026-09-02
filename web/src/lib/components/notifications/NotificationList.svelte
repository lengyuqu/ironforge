<script lang="ts">
  import { notifications, type NotificationItem } from '$lib/api/client.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT, formatDateTime } from '$lib/i18n';

  const t = createT();

  let {
    items,
    loading,
    onRefresh,
    onError,
  }: {
    items: NotificationItem[];
    loading: boolean;
    onRefresh: () => void | Promise<void>;
    onError: (message: string) => void;
  } = $props();

  let markingId = $state<number | null>(null);

  function eventIcon(type: string): string {
    switch (type) {
      case 'push': return '📦';
      case 'ci_triggered': return '🔧';
      case 'issue': return '❗';
      case 'pr': case 'pull_request': return '🔀';
      case 'review': return '👀';
      case 'pipeline': return '🔧';
      default: return '🔔';
    }
  }

  async function markRead(item: NotificationItem) {
    try {
      markingId = item.id;
      await notifications.markRead(item.id);
      await onRefresh();
    } catch (e: unknown) {
      onError(toErrorMessage(e, t('errors.load_failed', 'Failed to mark as read')));
    } finally {
      markingId = null;
    }
  }
</script>

{#if loading}
  <p>{t('common.loading')}</p>
{:else if items.length === 0}
  <div class="empty-state">
    <p>{t('notifications.empty')}</p>
  </div>
{:else}
  <div class="notif-list">
    {#each items as notif (notif.id)}
      <div class="notif-item" class:unread={!notif.is_read}>
        <div class="notif-icon">{eventIcon(notif.event_type)}</div>
        <div class="notif-content">
          <div class="notif-title">{notif.title}</div>
          {#if notif.body}<div class="notif-body">{notif.body}</div>{/if}
          <div class="notif-meta">
            <span class="notif-type">{notif.event_type}</span>
            <span class="notif-time">{formatDateTime(notif.created_at)}</span>
          </div>
        </div>
        <div class="notif-actions">
          {#if !notif.is_read}
            <button class="btn-xs" disabled={markingId === notif.id} onclick={() => markRead(notif)}>
              {markingId === notif.id ? t('common.loading') : t('notifications.mark_read')}
            </button>
          {/if}
        </div>
      </div>
    {/each}
  </div>
{/if}

<style>
  .empty-state {
    text-align: center;
    padding: 3rem;
    color: var(--text-secondary);
  }

  .notif-list {
    display: flex;
    flex-direction: column;
    gap: 0;
  }

  .notif-item {
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
    padding: 0.75rem 1rem;
    border-bottom: 1px solid var(--border);
    background: var(--bg-secondary);
  }

  .notif-item:first-child { border-radius: 8px 8px 0 0; }
  .notif-item:last-child { border-radius: 0 0 8px 8px; border-bottom: none; }

  .notif-item.unread { border-left: 3px solid var(--accent); }

  .notif-icon {
    font-size: 1.2rem;
    flex-shrink: 0;
    margin-top: 0.1rem;
  }

  .notif-content {
    flex: 1;
    min-width: 0;
  }

  .notif-title {
    color: var(--text-primary);
    font-weight: 500;
  }

  .notif-item.unread .notif-title { font-weight: 700; }

  .notif-body {
    color: var(--text-secondary);
    font-size: 0.85rem;
    margin-top: 0.2rem;
  }

  .notif-meta {
    display: flex;
    gap: 0.75rem;
    margin-top: 0.3rem;
    font-size: 0.8rem;
  }

  .notif-type {
    color: var(--accent);
    text-transform: uppercase;
    font-size: 0.7rem;
    font-weight: 600;
  }

  .notif-time { color: var(--text-secondary); }

  .notif-actions { flex-shrink: 0; }

  .btn-xs {
    background: transparent;
    color: var(--accent);
    border: 1px solid var(--accent);
    border-radius: 4px;
    padding: 0.2rem 0.5rem;
    cursor: pointer;
    font-size: 0.75rem;
  }

  .btn-xs:disabled { opacity: 0.6; cursor: not-allowed; }
</style>
