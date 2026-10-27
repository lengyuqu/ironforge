<script lang="ts">
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

  async function loadRepository() {
    try {
      loading = true;
      const response = await repos.get(owner, repo);
      repository = response;
    } catch (err: any) {
      error = err.message || t('errors.load_failed');
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

</style>
