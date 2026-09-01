<script lang="ts">
  // Labels settings page — orchestration layer: loads the label list and
  // manages modal visibility. Create/edit/delete are handled by the
  // self-contained modal components, which reload the list on success.
  import { page } from '$app/stores';
  import { labels } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import type { Label } from '$lib/types/entities';
  import LabelFormModal from '$lib/components/labels/LabelFormModal.svelte';
  import LabelDeleteModal from '$lib/components/labels/LabelDeleteModal.svelte';
  import LabelGrid from '$lib/components/labels/LabelGrid.svelte';

  let { data } = $props();
  const t = createT();

  const owner = $derived($page.params.owner!);
  const repo = $derived($page.params.repo!);

  let labelList = $state<Label[]>([]);
  let loading = $state(true);
  let error = $state('');

  // Modal state
  let showForm = $state(false);
  let editingLabel = $state<Label | null>(null);
  let deletingLabel = $state<Label | null>(null);

  $effect(() => {
    loadLabels();
  });

  async function loadLabels() {
    try {
      loading = true;
      error = '';
      labelList = await labels.list(owner, repo);
    } catch (e: unknown) {
      error = toErrorMessage(e, t('errors.load_failed', 'Load failed'));
    } finally {
      loading = false;
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

<div class="labels-page">
  <div class="page-header">
    <h1>{t('settings.labels')}</h1>
    <button class="btn btn-primary" onclick={openCreateForm}>
      + {t('settings.new_label')}
    </button>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
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
    <div class="loading">{t('common.loading')}</div>
  {:else if labelList.length === 0}
    <div class="empty-state">
      <p>{t('settings.no_labels')}</p>
    </div>
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

  .page-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 2rem;
  }

  h1 {
    font-size: 1.75rem;
    color: var(--text-primary);
    margin: 0;
  }

  .empty-state {
    padding: 3rem;
    text-align: center;
    color: var(--text-secondary);
    font-size: 0.95rem;
  }

  .loading {
    padding: 2rem;
    text-align: center;
    color: var(--text-secondary);
  }
</style>
