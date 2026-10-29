<script lang="ts">
  import { page } from '$app/stores';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import SuccessBanner from '$lib/components/SuccessBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import FormField from '$lib/components/FormField.svelte';
  import { ciRetention } from '$lib/api/client.svelte';

  const t = createT();
  const owner = $derived($page.params.owner!);
  const repo = $derived($page.params.repo!);

  let artifactDays = $state(30);
  let cacheDays = $state(7);
  let loading = $state(true);
  let error = $state('');
  let message = $state('');

  $effect(() => {
    owner;
    repo;
    load();
  });

  async function load() {
    try {
      loading = true;
      const policy = await ciRetention.get(owner, repo);
      artifactDays = policy.artifact_retention_days;
      cacheDays = policy.cache_retention_days;
      error = '';
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      loading = false;
    }
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    try {
      await ciRetention.update(owner, repo, { artifact_retention_days: artifactDays, cache_retention_days: cacheDays });
      message = t('settings.retention.saved');
      error = '';
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    }
  }

  async function cleanup() {
    try {
      const result = await ciRetention.cleanup(owner, repo);
      message = t('settings.retention.cleaned', { artifacts: result.artifacts_deleted, caches: result.caches_deleted });
      if (result.failures) message += ' ' + t('settings.retention.cleanup_failures', { count: result.failures });
      error = '';
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    }
  }
</script>

<svelte:head><title>{t('settings.retention.title')} · {owner}/{repo}</title></svelte:head>

<div class="settings-page">
  <header>
    <h1>{t('settings.retention.title')}</h1>
    <p>{t('settings.retention.desc')}</p>
  </header>

  {#if error}<ErrorBanner message={error} />{/if}
  {#if message}<SuccessBanner message={message} />{/if}

  {#if loading}
    <LoadingState />
  {:else}
    <form onsubmit={save}>
      <FormField id="artifact-days" label={t('settings.retention.artifact_days')}>
        {#snippet children({ id })}
          <input {id} type="number" min="1" max="3650" bind:value={artifactDays} required />
        {/snippet}
      </FormField>
      <FormField id="cache-days" label={t('settings.retention.cache_days')}>
        {#snippet children({ id })}
          <input {id} type="number" min="1" max="3650" bind:value={cacheDays} required />
        {/snippet}
      </FormField>
      <div class="form-actions">
        <button class="btn btn-primary">{t('settings.retention.save')}</button>
        <button type="button" class="btn" onclick={cleanup}>{t('settings.retention.cleanup')}</button>
      </div>
    </form>
  {/if}
</div>

<style>
  .settings-page {
    max-width: 760px;
  }

  header {
    margin-bottom: 24px;
  }

  header p {
    color: var(--text-secondary);
  }

  form {
    max-width: 520px;
  }
</style>
