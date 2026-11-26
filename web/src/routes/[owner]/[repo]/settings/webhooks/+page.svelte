<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import { page } from '$app/state';
  import { createT } from '$lib/i18n';
  import { createLatest } from '$lib/utils/latest';
  import { webhooks, type RepositoryWebhook } from '$lib/api/client.svelte';
  import WebhookForm from '$lib/components/settings/WebhookForm.svelte';
  import WebhookList from '$lib/components/settings/WebhookList.svelte';

  const t = createT();
  const latest = createLatest();
  const owner = $derived(page.params.owner!);
  const repo = $derived(page.params.repo!);

  let hooks = $state<RepositoryWebhook[]>([]);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    void loadWebhooks();
  });

  async function loadWebhooks() {
    const reqOwner = owner;
    const reqRepo = repo;
    const isCurrent = latest.begin();
    try {
      loading = true;
      error = '';
      const list = await webhooks.list(reqOwner, reqRepo);
      if (!isCurrent()) return;
      hooks = list;
    } catch (err) {
      if (!isCurrent()) return;
      error = toErrorMessage(err, t('settings.webhooks.load_failed', 'Failed to load webhooks'));
    } finally {
      if (isCurrent()) {
        loading = false;
      }
    }
  }
</script>

<svelte:head>
  <title>{t('settings.webhooks.title', 'Webhooks')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="webhooks-page">
  <div class="page-header">
    <div>
      <h1>{t('settings.webhooks.title', 'Webhooks')}</h1>
      <p>{t('settings.webhooks.desc', 'Send repository events to external services over HTTP.')}</p>
    </div>
  </div>

  {#if error}
    <ErrorBanner message={error} />
  {/if}

  <section class="settings-section">
    <h2>{t('settings.webhooks.create_title', 'Add webhook')}</h2>
    <WebhookForm {owner} {repo} onCreated={loadWebhooks} />
  </section>

  <section class="settings-section">
    <h2>{t('settings.webhooks.current', 'Configured webhooks')}</h2>
    {#if loading}
      <LoadingState />
    {:else}
      <WebhookList {owner} {repo} {hooks} onChanged={loadWebhooks} />
    {/if}
  </section>
</div>

<style>
  .webhooks-page {
    max-width: 900px;
  }

  h1 {
    margin: 0 0 0.5rem;
    color: var(--text-primary);
  }

  h2 {
    margin: 0 0 1rem;
    color: var(--text-primary);
  }

  p {
    margin: 0;
    color: var(--text-secondary);
    font-size: 0.95rem;
  }

</style>
