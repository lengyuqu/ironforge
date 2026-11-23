<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import { page } from '$app/stores';
  import { repos } from '$lib/api/client.svelte';
  import type { RepoInfo } from '$lib/types/entities';
  import RepoInfoSection from '$lib/components/settings/RepoInfoSection.svelte';
  import TransferSection from '$lib/components/settings/TransferSection.svelte';
  import DangerZoneSection from '$lib/components/settings/DangerZoneSection.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  const owner = $derived($page.params.owner!);
  const repo = $derived($page.params.repo!);

  let repository = $state<RepoInfo | null>(null);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    loadRepository();
  });

  let savingTemplate = $state(false);

  async function toggleTemplate(event: Event) {
    const target = event.currentTarget as HTMLInputElement;
    const next = target.checked;
    try {
      savingTemplate = true;
      await repos.setTemplate(owner, repo, next);
      await loadRepository();
    } catch (err) {
      // Put the checkbox back: the flag did not change on the server.
      target.checked = !next;
      error = toErrorMessage(err, t('errors.save_failed'));
    } finally {
      savingTemplate = false;
    }
  }

  async function loadRepository() {
    try {
      loading = true;
      const response = await repos.get(owner, repo);
      repository = response;
    } catch (err) {
      error = toErrorMessage(err, t('errors.load_failed'));
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head>
  <title>{t('settings.general')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="settings-page">
  <h1>{t('settings.general')}</h1>

  {#if loading}
    <LoadingState />
  {:else if error}
    <ErrorBanner message={error} />
  {:else if repository}
    <section class="section surface">
    <h2>{t('settings.template.title')}</h2>
    <label class="toggle-row">
      <input
        type="checkbox"
        checked={repository?.is_template ?? false}
        disabled={savingTemplate}
        onchange={toggleTemplate}
      />
      <span>
        <strong>{t('settings.template.enable')}</strong>
        <span class="hint">{t('settings.template.hint')}</span>
      </span>
    </label>
  </section>

  <RepoInfoSection {repository} />
    <TransferSection {owner} {repo} />
    <DangerZoneSection {owner} {repo} />
  {/if}
</div>

<style>
  .settings-page {
    max-width: 800px;
  }

  h1 {
    margin-bottom: 2rem;
    color: var(--text-primary);
  }

  .toggle-row { display: flex; align-items: flex-start; gap: 10px; font-size: 14px; cursor: pointer; }
  .toggle-row input[type='checkbox'] { width: 18px; height: 18px; margin-top: 2px; }
  .toggle-row .hint { display: block; font-size: 12px; opacity: 0.75; margin-top: 2px; }
</style>
