<script lang="ts">
  import { page } from '$app/stores';
  import { base } from '$app/paths';
  import { createT } from '$lib/i18n';

  const t = createT();

  let { children } = $props();

  const owner = $derived($page.params.owner!);
  const repo = $derived($page.params.repo!);
  const currentPath = $derived($page.url.pathname);

  const navItems = $derived([
    { path: `/${owner}/${repo}/settings`, label: t('settings.general'), icon: '⚙️' },
    { path: `/${owner}/${repo}/settings/labels`, label: t('settings.labels'), icon: '🏷️' }
  ]);
</script>

<div class="settings-layout">
  <aside class="sidebar">
    <nav>
      {#each navItems as item}
        <a 
          href={item.path} 
          class="nav-item"
          class:active={currentPath === item.path}
        >
          <span class="nav-icon">{item.icon}</span>
          <span class="nav-label">{item.label}</span>
        </a>
      {/each}
    </nav>
  </aside>
  
  <main class="content">
    <div class="breadcrumb">
      <a href="/{owner}/{repo}">{owner}/{repo}</a>
      <span class="separator">/</span>
      <span>{t('settings.title')}</span>
      {#if currentPath.includes('/labels')}
        <span class="separator">/</span>
        <span>{t('settings.labels')}</span>
      {/if}
    </div>
    
    {@render children()}
  </main>
</div>

<style>
  .settings-layout {
    display: flex;
    gap: 2rem;
    max-width: 1200px;
    margin: 0 auto;
    padding: 2rem;
    min-height: calc(100vh - 60px);
  }
  
  .sidebar {
    width: 200px;
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
    gap: 0.75rem;
    padding: 0.75rem 1rem;
    border-radius: 6px;
    color: var(--text-primary);
    text-decoration: none;
    transition: all 0.2s;
    border-left: 3px solid transparent;
  }
  
  .nav-item:hover {
    background: var(--bg-secondary);
  }
  
  .nav-item.active {
    color: var(--accent);
    border-left-color: var(--accent);
    background: var(--bg-secondary);
    font-weight: 600;
  }
  
  .nav-icon {
    font-size: 1.1rem;
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
</style>
