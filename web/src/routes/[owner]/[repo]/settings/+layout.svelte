<script lang="ts">
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { isAuthReady, isLoggedIn } from '$lib/stores/auth.svelte';
  import { createT } from '$lib/i18n';
  import Icon, { type IconName } from '$lib/components/Icon.svelte';

  const t = createT();

  let { children } = $props();

  // F-003: Auth guard for all settings sub-pages
  $effect(() => {
    if (!isAuthReady()) return;
    if (!isLoggedIn()) {
      goto('/login');
    }
  });

  const owner = $derived($page.params.owner!);
  const repo = $derived($page.params.repo!);
  const currentPath = $derived($page.url.pathname);

  const navItems: Array<{ path: string; label: string; icon: IconName }> = $derived([
    { path: `/${owner}/${repo}/settings`, label: t('settings.general'), icon: 'settings' },
    { path: `/${owner}/${repo}/settings/labels`, label: t('settings.labels'), icon: 'tag' },
    { path: `/${owner}/${repo}/settings/milestones`, label: t('settings.milestones'), icon: 'target' },
    { path: `/${owner}/${repo}/settings/branches`, label: t('settings.branch_protection.title'), icon: 'shield' },
    { path: `/${owner}/${repo}/settings/deploy-keys`, label: t('settings.deploy_keys.title', 'Deploy keys'), icon: 'key' },
    { path: `/${owner}/${repo}/settings/ci-secrets`, label: t('settings.ci_secrets.title', 'CI secrets'), icon: 'lock' },
    { path: `/${owner}/${repo}/settings/environments`, label: t('settings.environments.title', 'Environments'), icon: 'rocket' },
    { path: `/${owner}/${repo}/settings/retention`, label: t('settings.retention.title', 'CI retention'), icon: 'trash' },
    { path: `/${owner}/${repo}/settings/tags`, label: t('settings.tag_protection.title', 'Tag protection'), icon: 'tag' },
    { path: `/${owner}/${repo}/settings/mirror`, label: t('settings.mirror.title'), icon: 'repeat' },
    { path: `/${owner}/${repo}/settings/webhooks`, label: t('settings.webhooks.title', 'Webhooks'), icon: 'bell' },
    { path: `/${owner}/${repo}/settings/collaborators`, label: t('settings.collaborators.title'), icon: 'users' },
    { path: `/${owner}/${repo}/settings/runners`, label: t('admin.runners.title'), icon: 'cpu' },
  ]);

  const currentSection = $derived(navItems.find((item) => item.path === currentPath));
</script>

<div class="settings-layout">
  <aside class="sidebar">
    <nav aria-label={t('settings.nav_label')}>
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
    <div class="breadcrumb">
      <a href={`/${owner}/${repo}`}>{owner}/{repo}</a>
      <span class="separator">/</span>
      <span>{t('settings.title')}</span>
      {#if currentSection && currentSection.path !== `/${owner}/${repo}/settings`}
        <span class="separator">/</span>
        <span>{currentSection.label}</span>
      {/if}
    </div>

    {@render children()}
  </div>
</div>

<style>
  .settings-layout {
    display: flex;
    gap: 2rem;
    min-height: calc(100vh - 220px);
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

  .nav-label {
    font-size: 0.9rem;
  }

  .content {
    flex: 1;
    min-width: 0;
  }

  .breadcrumb {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 2rem;
    font-size: 0.9rem;
    color: var(--text-secondary);
  }

  .breadcrumb a {
    color: var(--accent);
    text-decoration: none;
  }

  .breadcrumb a:hover {
    text-decoration: underline;
  }

  .separator {
    color: var(--text-muted);
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
