<script lang="ts">
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { isAuthReady, isLoggedIn } from '$lib/stores/auth.svelte';
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
    }
  });

  const currentPath = $derived(page.url.pathname);

  const navItems: Array<{ path: string; label: string; icon: IconName }> = $derived([
    { path: '/settings/security', label: t('nav.security'), icon: 'shield' },
    { path: '/settings/ssh-keys', label: t('nav.ssh_keys'), icon: 'key' },
    { path: '/settings/tokens', label: t('nav.access_tokens'), icon: 'lock' },
    { path: '/settings/credentials', label: t('nav.remote_credentials'), icon: 'globe' },
  ]);
</script>

<div class="page-container">
  <div class="settings-layout">
    <aside class="sidebar">
      <nav aria-label={t('settings.user_nav_label')}>
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
  .settings-layout {
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
    .settings-layout {
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
