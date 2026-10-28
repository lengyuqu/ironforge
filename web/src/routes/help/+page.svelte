<script lang="ts">
  import { createT } from '$lib/i18n';

  const t = createT();
</script>

<svelte:head>
  <title>{t('help.title')} - IronForge</title>
</svelte:head>

<div class="help-page">
  <header class="help-header">
    <h1>{t('help.title')}</h1>
    <p>{t('help.subtitle')}</p>
  </header>

  <section class="help-section">
    <h2>{t('help.clone_title')}</h2>
    <div class="command-list">
      <code>git clone http://localhost:8080/git/OWNER/REPO</code>
      <code>git clone ssh://git@localhost:2222/OWNER/REPO</code>
    </div>
  </section>

  <section class="help-section">
    <h2>{t('help.tokens_title')}</h2>
    <p>{t('help.tokens_desc')}</p>
    <a href="/settings/tokens" class="link-button">{t('help.tokens_manage')}</a>
  </section>

  <section class="help-section">
    <h2>{t('help.find_title')}</h2>
    <div class="quick-links">
      <a href="/explore">{t('help.explore')}</a>
      <a href="/search">{t('help.search')}</a>
      <a href="/imports">{t('help.import')}</a>
    </div>
  </section>
</div>

<style>
  .help-page {
    max-width: 920px;
    margin: 0 auto;
    padding: 40px 24px 64px;
  }

  .help-header {
    border-bottom: 1px solid var(--border);
    padding-bottom: 20px;
    margin-bottom: 28px;
  }

  .help-header h1 {
    margin: 0 0 8px;
    font-size: 32px;
  }

  .help-header p,
  .help-section p {
    color: var(--text-secondary);
    line-height: 1.6;
  }

  .help-section {
    padding: 20px 0;
    border-bottom: 1px solid var(--border);
  }

  .help-section h2 {
    margin: 0 0 12px;
    font-size: 20px;
  }

  .command-list {
    display: grid;
    gap: 10px;
  }

  code {
    display: block;
    overflow-x: auto;
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-secondary);
    color: var(--text-primary);
  }

  .quick-links {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
  }

  .quick-links a,
  .link-button {
    color: var(--accent);
    font-weight: 600;
    text-decoration: none;
  }

  .quick-links a:hover,
  .link-button:hover {
    text-decoration: underline;
  }
</style>
