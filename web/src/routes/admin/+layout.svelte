<script lang="ts">
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { isAuthReady, isLoggedIn, isAdmin } from '$lib/stores/auth.svelte';
  import { createT } from '$lib/i18n';
  import Icon, { type IconName } from '$lib/components/Icon.svelte';
  import type { Snippet } from 'svelte';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();
  const t = createT();

  $effect(() => {
    if (!isAuthReady()) return;
    if (!isLoggedIn()) {
      goto('/login');
      return;
    }
    if (!isAdmin()) {
      goto('/dashboard');
    }
  });

  const currentPath = $derived($page.url.pathname);

  const navItems: Array<{ path: string; label: string; icon: IconName }> = $derived([
    { path: '/admin', label: t('admin.overview'), icon: 'activity' },
    { path: '/admin/users', label: t('admin.users.title'), icon: 'users' },
    { path: '/admin/orgs', label: t('admin.orgs.title'), icon: 'building' },
    { path: '/admin/audit', label: t('admin.audit.title'), icon: 'clipboard' },
    { path: '/admin/runners', label: t('admin.runners.title'), icon: 'cpu' },
    { path: '/admin/settings', label: t('admin.settings.title'), icon: 'settings' },
  ]);
</script>

<div class="page-container">
  <div class="admin-layout">
    <aside class="sidebar">
      <nav aria-label={t('admin.nav_label')}>
        {#each navItems as item (item.path)}
          <a
            href={item.path}
            class="nav-item"
            class:active={currentPath === item.path}
            aria-current={currentPath === item.path ? 'page' : undefined}
          >
            <span class="nav-icon"><Icon name={item.icon} size={16} /></span>
            <span class="nav-label">{item.label}</span>
          </a>
        {/each}
      </nav>
    </aside>

    <div class="content">
      {@render children()}
    </div>
  </div>
</div>

<style>
  .admin-layout {
    display: flex;
    gap: 2rem;
    min-height: calc(100vh - 180px);
  }

  .sidebar {
    width: 210px;
    flex-shrink: 0;
  }

  .sidebar nav {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 0.65rem;
    padding: 0.65rem 0.9rem;
    border-radius: var(--radius);
    color: var(--text-secondary);
    text-decoration: none;
    transition: background-color 0.15s, color 0.15s;
    border-left: 3px solid transparent;
    font-size: 0.9rem;
  }

  .nav-item:hover {
    background: var(--bg-secondary);
    color: var(--text-primary);
    text-decoration: none;
  }

  .nav-item.active {
    color: var(--accent);
    border-left-color: var(--accent);
    background: var(--bg-secondary);
    font-weight: 600;
  }

  .nav-icon {
    display: inline-flex;
    align-items: center;
  }

  .content {
    flex: 1;
    min-width: 0;
  }

  @media (max-width: 720px) {
    .admin-layout {
      flex-direction: column;
      gap: 1rem;
    }

    .sidebar {
      width: 100%;
    }

    .sidebar nav {
      flex-direction: row;
      flex-wrap: wrap;
    }
  }
</style>
