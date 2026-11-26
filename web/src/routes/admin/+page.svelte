<script lang="ts">
  import { createT } from '$lib/i18n';
  import Icon, { type IconName } from '$lib/components/Icon.svelte';

  const t = createT();

  const cards: Array<{ href: string; icon: IconName; titleKey: string; descKey: string }> = [
    { href: '/admin/users', icon: 'users', titleKey: 'admin.users.title', descKey: 'admin.users.desc' },
    { href: '/admin/orgs', icon: 'building', titleKey: 'admin.orgs.title', descKey: 'admin.orgs.desc' },
    { href: '/admin/audit', icon: 'clipboard', titleKey: 'admin.audit.title', descKey: 'admin.audit.desc' },
    { href: '/admin/runners', icon: 'cpu', titleKey: 'admin.runners.title', descKey: 'admin.runners.desc' },
    { href: '/admin/settings', icon: 'settings', titleKey: 'admin.settings.title', descKey: 'admin.settings.desc' },
  ];
</script>

<svelte:head>
  <title>{t('admin.title')} · IronForge</title>
</svelte:head>

<div class="header">
  <h1>{t('admin.title')}</h1>
  <p class="subtitle">{t('admin.subtitle')}</p>
</div>

<div class="cards">
  {#each cards as card (card.href)}
    <a href={card.href} class="card">
      <div class="card-icon"><Icon name={card.icon} size={24} /></div>
      <div class="card-body">
        <h2>{t(card.titleKey)}</h2>
        <p>{t(card.descKey)}</p>
      </div>
      <div class="card-arrow" aria-hidden="true">→</div>
    </a>
  {/each}
</div>

<style>
  .header {
    margin-bottom: 2rem;
  }

  h1 {
    color: var(--text-primary);
    margin: 0 0 0.5rem;
  }

  .subtitle {
    color: var(--text-secondary);
    margin: 0;
  }

  .cards {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .card {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 1.25rem 1.5rem;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 10px;
    text-decoration: none;
    color: inherit;
    transition: border-color 0.15s, background 0.15s;
  }

  .card:hover {
    border-color: var(--accent);
    background: var(--bg-hover);
    text-decoration: none;
  }

  .card-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    border-radius: var(--radius-lg);
    background: var(--bg-tertiary);
    color: var(--accent);
    flex-shrink: 0;
  }

  .card-body {
    flex: 1;
  }

  .card-body h2 {
    margin: 0 0 0.25rem;
    font-size: 1.05rem;
    color: var(--text-primary);
  }

  .card-body p {
    margin: 0;
    font-size: 0.875rem;
    color: var(--text-secondary);
  }

  .card-arrow {
    color: var(--text-secondary);
    font-size: 1.25rem;
    flex-shrink: 0;
  }
</style>
