<script lang="ts">
  import { page } from '$app/stores';
  import { createT } from '$lib/i18n';
  import { webhooks, type RepositoryWebhook } from '$lib/api/client.svelte';
  import WebhookForm from '$lib/components/settings/WebhookForm.svelte';
  import WebhookList from '$lib/components/settings/WebhookList.svelte';

  const t = createT();
  const owner = $derived($page.params.owner!);
  const repo = $derived($page.params.repo!);

  let hooks = $state<RepositoryWebhook[]>([]);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    loadWebhooks();
  });

  async function loadWebhooks() {
    try {
      loading = true;
      error = '';
      hooks = await webhooks.list(owner, repo);
    } catch (err: any) {
      error = err.message || t('settings.webhooks.load_failed', 'Failed to load webhooks');
    } finally {
      loading = false;
    }
  }
</script>

<div class="webhooks-page">
  <div class="page-header">
    <div>
      <h1>{t('settings.webhooks.title', 'Webhooks')}</h1>
      <p>{t('settings.webhooks.desc', 'Send repository events to external services over HTTP.')}</p>
    </div>
  </div>

  {#if error}
    <div class="error-box">{error}</div>
  {/if}

  <section class="section">
    <h2>{t('settings.webhooks.create_title', 'Add webhook')}</h2>
    <WebhookForm {owner} {repo} onCreated={loadWebhooks} />
  </section>

  <section class="section">
    <h2>{t('settings.webhooks.current', 'Configured webhooks')}</h2>
    {#if loading}
      <div class="loading">{t('common.loading')}</div>
    {:else}
      <WebhookList {owner} {repo} {hooks} onChanged={loadWebhooks} />
    {/if}
  </section>
</div>

<style>
  .webhooks-page {
    max-width: 900px;
  }

  .page-header {
    margin-bottom: 2rem;
  }

  h1 {
    font-size: 1.75rem;
    margin: 0 0 0.5rem;
    color: var(--text-primary);
  }

  h2 {
    font-size: 1.1rem;
    margin: 0 0 1rem;
    color: var(--text-primary);
  }

  p {
    margin: 0;
    color: var(--text-secondary);
    font-size: 0.95rem;
  }

  .section {
    margin-bottom: 2.5rem;
    padding-bottom: 2rem;
    border-bottom: 1px solid var(--border);
  }

  .loading {
    padding: 2rem;
    text-align: center;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border-radius: 6px;
  }

  .error-box {
    padding: 0.75rem;
    border-radius: 6px;
    font-size: 0.9rem;
    margin-bottom: 1rem;
    background: rgba(255, 0, 0, 0.1);
    border: 1px solid var(--red, #ff4444);
    color: var(--red, #ff4444);
  }
</style>
