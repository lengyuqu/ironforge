<script lang="ts">
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
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
      case 'collab_question': return '💬';
      case 'collab_task_blocked': return '⛔';
      default: return '🔔';
    }
  }

  /** Where to go to act on a notification, when it points at something. */
  function targetOf(item: NotificationItem): string | null {
    if (item.event_type.startsWith('collab_') && item.repo) {
      return `/${item.repo}/collaboration`;
    }
    return null;
  }

  // Collaboration notifications carry the agent-written title and a fixed English
  // body for API clients; the web client wraps the title in its own wording.
  const COLLAB_TEXT: Record<string, { type: string; hint: string }> = {
    collab_question: { type: 'notifications.collab_question_type', hint: 'notifications.collab_question_hint' },
    collab_task_blocked: { type: 'notifications.collab_task_blocked_type', hint: 'notifications.collab_task_blocked_hint' },
  };

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
  <LoadingState />
{:else if items.length === 0}
  <EmptyState title={t('notifications.empty')} />
{:else}
  <div class="notif-list">
    {#each items as notif (notif.id)}
      <div class="notif-item" class:unread={!notif.is_read}>
        <div class="notif-icon">{eventIcon(notif.event_type)}</div>
        <div class="notif-content">
          <div class="notif-title">{notif.title}</div>
          {#if COLLAB_TEXT[notif.event_type]}
            {#if notif.repo}
              <div class="notif-body">{t(COLLAB_TEXT[notif.event_type].hint, { repo: notif.repo })}</div>
            {/if}
          {:else if notif.body}
            <div class="notif-body">{notif.body}</div>
          {/if}
          <div class="notif-meta">
            <span class="notif-type">{COLLAB_TEXT[notif.event_type] ? t(COLLAB_TEXT[notif.event_type].type) : notif.event_type}</span>
            {#if targetOf(notif)}
              <a class="notif-open" href={targetOf(notif)}>{t('notifications.open')}</a>
            {/if}
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
  .notif-open { color: var(--accent); }

  .notif-actions { flex-shrink: 0; }
</style>
