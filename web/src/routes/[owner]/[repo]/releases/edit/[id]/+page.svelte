<script lang="ts">
  // Edit-release page — orchestration layer: loads the release and hands
  // it to ReleaseForm (edit mode: tag locked, no target picker). Submit
  // handling lives inside the form; this page only routes back on success.
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import ReleaseForm from '$lib/components/releases/ReleaseForm.svelte';
  import { releases } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { createLatest } from '$lib/utils/latest';
  import { toErrorMessage } from '$lib/utils/error';
  import type { Release } from '$lib/types/entities';

  const t = createT();
  const latest = createLatest();

  const owner = $derived(page.params.owner!);
  const repo = $derived(page.params.repo!);
  const releaseId = $derived(parseInt(page.params.id!, 10));

  let loading = $state(true);
  let error = $state('');
  let notFound = $state(false);
  let release = $state<Release | null>(null);

  $effect(() => {
    if (!Number.isFinite(releaseId) || releaseId <= 0) {
      latest.invalidate();
      notFound = true;
      loading = false;
      return;
    }
    notFound = false;
    void loadRelease();
  });

  async function loadRelease() {
    const reqOwner = owner;
    const reqRepo = repo;
    const reqReleaseId = releaseId;
    const isCurrent = latest.begin();
    loading = true;
    error = '';
    try {
      const data = await releases.get(reqOwner, reqRepo, reqReleaseId);
      if (!isCurrent()) return;
      release = data;
    } catch (e: unknown) {
      if (!isCurrent()) return;
      error = toErrorMessage(e, t('errors.load_failed', 'Load failed'));
    } finally {
      if (isCurrent()) {
        loading = false;
      }
    }
  }

  function handleSaved() {
    goto(`/${owner}/${repo}/releases`);
  }
</script>

<svelte:head>
  <title>{t('releases.edit_title')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-header">
  <h1>{t('releases.edit')} #{releaseId}</h1>
</div>

{#if notFound}
  <EmptyState title={t('releases.invalid_id', 'Invalid release id.')}>
    <a href={`/${owner}/${repo}/releases`} class="btn-primary">{t('releases.title')}</a>
  </EmptyState>
{:else if error}
  <ErrorBanner message={error} />
{/if}

{#if loading}
  <LoadingState />
{:else if release}
  <ReleaseForm {owner} {repo} {release} onSaved={handleSaved} />
{/if}