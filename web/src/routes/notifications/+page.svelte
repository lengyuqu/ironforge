<script lang="ts">
  import { onMount } from 'svelte';
  import {
    notifications,
    connectNotificationWebSocket,
    disconnectNotificationWebSocket,
    type NotificationItem,
  } from '$lib/api/client.svelte';
  import { getUser, isLoggedIn } from '$lib/stores/auth.svelte';
  import NotificationList from '$lib/components/notifications/NotificationList.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  let notifs = $state<NotificationItem[]>([]);
  let unreadCount = $state(0);
  let loading = $state(true);
  let filterUnread = $state(false);
  let wsConnected = $state(false);
  let error = $state('');

  async function load() {
    loading = true;
    try {
      const userId = getUser()?.id;
      notifs = (await notifications.list(userId, filterUnread)).data;
      const countData = await notifications.unreadCount(userId);
      unreadCount = countData.unread_count || 0;
    } catch (e) {
      error = (e as Error).message || t('errors.load_failed');
    } finally {
      loading = false;
    }
  }

  async function markAllRead() {
    try {
      const userId = getUser()?.id;
      await notifications.markAllRead(userId);
      await load();
    } catch (e) {
      error = (e as Error).message || t('errors.load_failed');
    }
  }

  function setupWebSocket() {
    if (!isLoggedIn()) return;
    const ws = connectNotificationWebSocket(
      (event) => {
        if (event.event_type === 'push' || event.event_type === 'ci_triggered') {
          unreadCount++;
          load();
        }
      },
      () => {
        wsConnected = false;
      },
    );
    if (ws) {
      wsConnected = true;
      ws.addEventListener('open', () => { wsConnected = true; });
      ws.addEventListener('close', () => { wsConnected = false; });
    }
  }

  load();
  onMount(() => {
    setupWebSocket();
    return () => disconnectNotificationWebSocket();
  });
</script>

<div class="container">
  <div class="header">
    <h1>{t('notifications.title')} {unreadCount > 0 ? `(${unreadCount})` : ''}</h1>
    <div class="actions">
      <span class="ws-status" class:connected={wsConnected}>
        {wsConnected ? `🟢 ${t('nav.live')}` : `🔴 ${t('nav.offline')}`}
      </span>
      <label class="filter">
        <input type="checkbox" bind:checked={filterUnread} onchange={load} />
        {t('notifications.unread_only')}
      </label>
      {#if unreadCount > 0}
        <button class="btn-sm" onclick={markAllRead}>{t('notifications.mark_all_read')}</button>
      {/if}
    </div>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <NotificationList
    items={notifs}
    {loading}
    onRefresh={load}
    onError={(message) => (error = message)}
  />

  {#if !loading && notifs.length === 0 && wsConnected}
    <p class="hint">{t('notifications.hint')}</p>
  {/if}
</div>

<style>
  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 1.5rem;
    flex-wrap: wrap;
    gap: 0.75rem;
  }

  h1 { color: var(--text-primary); margin: 0; }
  .actions { display: flex; align-items: center; gap: 1rem; flex-wrap: wrap; }
  .ws-status { font-size: 0.8rem; color: var(--text-secondary); }
  .ws-status.connected { color: #22c55e; }

  .filter {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    color: var(--text-secondary);
    font-size: 0.9rem;
    cursor: pointer;
  }

  .btn-sm {
    background: var(--accent);
    color: white;
    border: none;
    border-radius: 4px;
    padding: 0.4rem 0.8rem;
    cursor: pointer;
    font-size: 0.85rem;
  }

  .hint {
    font-size: 0.85rem;
    color: var(--text-secondary);
    margin-top: 0.5rem;
  }

  .error-banner {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 10px 12px;
    border-radius: var(--radius);
    margin-bottom: 16px;
  }
</style>
