<script lang="ts">
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import Pagination from '$lib/components/Pagination.svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
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
  import { setUnreadCount } from '$lib/stores/unread.svelte';
  import { createLatest } from '$lib/utils/latest';
  import { toErrorMessage } from '$lib/utils/error';
  import { buildQueryHref, readPageParam } from '$lib/utils/urlQuery';

  const t = createT();
  const PER_PAGE = 20;

  let notifs = $state<NotificationItem[]>([]);
  let total = $state(0);
  let unreadCount = $state(0);
  let loading = $state(true);
  let filterUnread = $derived(page.url.searchParams.get('unread') === '1');
  let currentPage = $derived(readPageParam(page.url));
  let wsConnected = $state(false);
  let error = $state('');

  const latest = createLatest();

  $effect(() => {
    void load();
  });

  async function load() {
    const reqUnread = filterUnread;
    const reqPage = currentPage;
    const userId = getUser()?.id;
    const isCurrent = latest.begin();
    loading = true;
    error = '';
    try {
      const [listResult, countData] = await Promise.all([
        notifications.list(userId, reqUnread, reqPage, PER_PAGE),
        notifications.unreadCount(userId),
      ]);
      if (!isCurrent()) return;
      notifs = listResult.data;
      total = listResult.pagination?.total ?? listResult.data.length;
      unreadCount = countData.unread_count || 0;
      setUnreadCount(unreadCount);
      if (reqPage > 1 && listResult.data.length === 0 && total > 0) {
        void goto(buildQueryHref(page.url, { page: null }), {
          replaceState: true,
          keepFocus: true,
          noScroll: true,
        });
      }
    } catch (e) {
      if (!isCurrent()) return;
      error = toErrorMessage(e, t('errors.load_failed'));
    } finally {
      if (isCurrent()) {
        loading = false;
      }
    }
  }

  function toggleUnreadFilter(next: boolean) {
    void goto(buildQueryHref(page.url, { unread: next ? 1 : null, page: null }), {
      keepFocus: true,
      noScroll: true,
    });
  }

  function changePage(nextPage: number) {
    void goto(buildQueryHref(page.url, { page: nextPage === 1 ? null : nextPage }), {
      keepFocus: true,
    });
  }

  async function markAllRead() {
    try {
      const userId = getUser()?.id;
      await notifications.markAllRead(userId);
      await load();
    } catch (e) {
      error = toErrorMessage(e, t('errors.load_failed'));
    }
  }

  function setupWebSocket() {
    if (!isLoggedIn()) return;
    const ws = connectNotificationWebSocket(
      (event) => {
        if (
          event.event_type === 'push' ||
          event.event_type === 'ci_triggered' ||
          event.event_type.startsWith('collab_')
        ) {
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

  onMount(() => {
    setupWebSocket();
    return () => disconnectNotificationWebSocket();
  });
</script>

<svelte:head>
  <title>{t('notifications.title')} · IronForge</title>
</svelte:head>

<div class="page-container">
  <div class="header">
    <h1>{t('notifications.title')} {unreadCount > 0 ? `(${unreadCount})` : ''}</h1>
    <div class="actions">
      <span class="ws-status" class:connected={wsConnected}>
        {wsConnected ? `🟢 ${t('nav.live')}` : `🔴 ${t('nav.offline')}`}
      </span>
      <label class="filter">
        <input
          type="checkbox"
          checked={filterUnread}
          onchange={(e) => toggleUnreadFilter(e.currentTarget.checked)}
        />
        {t('notifications.unread_only')}
      </label>
      {#if unreadCount > 0}
        <button class="btn-sm" onclick={markAllRead}>{t('notifications.mark_all_read')}</button>
      {/if}
    </div>
  </div>

  {#if error}
    <ErrorBanner message={error} />
  {/if}

  <NotificationList
    items={notifs}
    {loading}
    onRefresh={load}
    onError={(message) => (error = message)}
  />

  <Pagination {total} page={currentPage} perPage={PER_PAGE} onPageChange={changePage} />

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
  .ws-status.connected { color: var(--green); }

  .filter {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    color: var(--text-secondary);
    font-size: 0.9rem;
    cursor: pointer;
  }

  .hint {
    font-size: 0.85rem;
    color: var(--text-secondary);
    margin-top: 0.5rem;
  }

</style>
