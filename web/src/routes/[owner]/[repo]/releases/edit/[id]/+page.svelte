<script lang="ts">
  // Edit-release page — orchestration layer: loads the release and hands
  // it to ReleaseForm (edit mode: tag locked, no target picker). Submit
  // handling lives inside the form; this page only routes back on success.
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import ReleaseForm from '$lib/components/releases/ReleaseForm.svelte';
  import { releases } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import type { Release } from '$lib/types/entities';

  const t = createT();

  const owner = $derived($page.params.owner!);
  const repo = $derived($page.params.repo!);
  const releaseId = $derived(parseInt($page.params.id!, 10));

  let loading = $state(true);
  let error = $state('');
  let notFound = $state(false);
  let release = $state<Release | null>(null);

  $effect(() => {
    if (!Number.isFinite(releaseId) || releaseId <= 0) {
      notFound = true;
      loading = false;
      return;
    }
    loadRelease();
  });

  async function loadRelease() {
    loading = true;
    error = '';
    try {
      release = await releases.get(owner, repo, releaseId);
    } catch (e: unknown) {
      error = toErrorMessage(e, t('errors.load_failed', 'Load failed'));
    } finally {
      loading = false;
    }
  }

  function handleSaved() {
    goto(`/${owner}/${repo}/releases`);
  }
</script>

<svelte:head>
  <title>Edit Release · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="releases" />

  <div class="page-header">
    <h1>{t('releases.edit')} #{releaseId}</h1>
  </div>

  {#if notFound}
    <div class="empty">
      <p>{t('releases.invalid_id', 'Invalid release id.')}</p>
      <a href={`/${owner}/${repo}/releases`} class="btn-primary">{t('releases.title')}</a>
    </div>
  {:else if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="loading-text">{t('common.loading')}</p>
  {:else if release}
    <ReleaseForm {owner} {repo} {release} onSaved={handleSaved} />
  {/if}
</div>

<style>
  .page-header {
    margin-bottom: 24px;
  }

  h1 {
    font-size: 24px;
    font-weight: 600;
  }

  .loading-text {
    color: var(--text-secondary);
    text-align: center;
    padding: 48px;
  }

  .empty {
    text-align: center;
    padding: 48px;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .error-banner {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 10px 12px;
    border-radius: var(--radius);
    margin-bottom: 16px;
  }

  .btn-primary {
    padding: 6px 16px;
    background: var(--orange);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    text-decoration: none;
    display: inline-block;
    margin-top: 12px;
  }
</style>
