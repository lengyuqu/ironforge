<script lang="ts">
  // Labels settings page — orchestration layer: loads the label list and
  // manages modal visibility. Create/edit/delete are handled by the
  // self-contained modal components, which reload the list on success.
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { page } from '$app/state';
  import { labels } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { createLatest } from '$lib/utils/latest';
  import { toErrorMessage } from '$lib/utils/error';
  import type { Label } from '$lib/types/entities';
  import LabelFormModal from '$lib/components/labels/LabelFormModal.svelte';
  import LabelDeleteModal from '$lib/components/labels/LabelDeleteModal.svelte';
  import LabelGrid from '$lib/components/labels/LabelGrid.svelte';

  let { data } = $props();
  const t = createT();
  const latest = createLatest();

  const owner = $derived(page.params.owner!);
  const repo = $derived(page.params.repo!);

  let labelList = $state<Label[]>([]);
  let loading = $state(true);
  let error = $state('');

  // Modal state
  let showForm = $state(false);
  let editingLabel = $state<Label | null>(null);
  let deletingLabel = $state<Label | null>(null);

  $effect(() => {
    void loadLabels();
  });

  async function loadLabels() {
    const reqOwner = owner;
    const reqRepo = repo;
    const isCurrent = latest.begin();
    try {
      loading = true;
      error = '';
      const list = await labels.list(reqOwner, reqRepo);
      if (!isCurrent()) return;
      labelList = list;
    } catch (e: unknown) {
      if (!isCurrent()) return;
      error = toErrorMessage(e, t('errors.load_failed', 'Load failed'));
    } finally {
      if (isCurrent()) {
        loading = false;
      }
    }
  }

  function openCreateForm() {
    editingLabel = null;
    showForm = true;
  }

  function openEditForm(label: Label) {
    editingLabel = label;
    showForm = true;
  }

  function closeForm() {
    showForm = false;
    editingLabel = null;
  }
</script>

<svelte:head>
  <title>{t('settings.labels')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="labels-page">
  <div class="page-header">
    <h1>{t('settings.labels')}</h1>
    <button class="btn btn-primary" onclick={openCreateForm}>
      + {t('settings.new_label')}
    </button>
  </div>

  {#if error}
    <ErrorBanner message={error} />
  {/if}

  {#if showForm}
    <LabelFormModal
      {owner}
      {repo}
      label={editingLabel}
      onClose={closeForm}
      onSaved={loadLabels}
    />
  {/if}

  {#if deletingLabel}
    <LabelDeleteModal
      {owner}
      {repo}
      label={deletingLabel}
      onClose={() => (deletingLabel = null)}
      onDeleted={loadLabels}
    />
  {/if}

  {#if loading}
    <LoadingState />
  {:else if labelList.length === 0}
    <EmptyState title={t('settings.no_labels')} />
  {:else}
    <LabelGrid
      items={labelList}
      onEdit={openEditForm}
      onDelete={(label) => (deletingLabel = label)}
    />
  {/if}
</div>

<style>
  .labels-page {
    max-width: 800px;
  }

  h1 {
    color: var(--text-primary);
    margin: 0;
  }
</style>
