<script lang="ts">
  // Milestones settings page — orchestration layer: loads the milestone
  // list and manages modal visibility. Create/edit/delete are handled by
  // the self-contained modal components, which reload the list on success.
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { page } from '$app/stores';
  import { milestones } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import type { Milestone } from '$lib/types/entities';
  import MilestoneFormModal from '$lib/components/milestones/MilestoneFormModal.svelte';
  import MilestoneDeleteModal from '$lib/components/milestones/MilestoneDeleteModal.svelte';
  import MilestoneGrid from '$lib/components/milestones/MilestoneGrid.svelte';

  let { data } = $props();
  const t = createT();

  const owner = $derived($page.params.owner!);
  const repo = $derived($page.params.repo!);

  let milestoneList = $state<Milestone[]>([]);
  let loading = $state(true);
  let error = $state('');

  // Modal state
  let showForm = $state(false);
  let editingMilestone = $state<Milestone | null>(null);
  let deletingMilestone = $state<Milestone | null>(null);

  $effect(() => {
    loadMilestones();
  });

  async function loadMilestones() {
    try {
      loading = true;
      error = '';
      milestoneList = await milestones.list(owner, repo);
    } catch (e: unknown) {
      error = toErrorMessage(e, t('errors.load_failed', 'Load failed'));
    } finally {
      loading = false;
    }
  }

  function openCreateForm() {
    editingMilestone = null;
    showForm = true;
  }

  function openEditForm(milestone: Milestone) {
    editingMilestone = milestone;
    showForm = true;
  }

  function closeForm() {
    showForm = false;
    editingMilestone = null;
  }
</script>

<div class="milestones-page">
  <div class="page-header">
    <h1>{t('settings.milestones')}</h1>
    <button class="btn btn-primary" onclick={openCreateForm}>
      + {t('settings.new_milestone')}
    </button>
  </div>

  {#if error}
    <ErrorBanner message={error} />
  {/if}

  {#if showForm}
    <MilestoneFormModal
      {owner}
      {repo}
      milestone={editingMilestone}
      onClose={closeForm}
      onSaved={loadMilestones}
    />
  {/if}

  {#if deletingMilestone}
    <MilestoneDeleteModal
      {owner}
      {repo}
      milestone={deletingMilestone}
      onClose={() => (deletingMilestone = null)}
      onDeleted={loadMilestones}
    />
  {/if}

  {#if loading}
    <LoadingState />
  {:else if milestoneList.length === 0}
    <EmptyState title={t('settings.no_milestones')} />
  {:else}
    <MilestoneGrid
      items={milestoneList}
      onEdit={openEditForm}
      onDelete={(milestone) => (deletingMilestone = milestone)}
    />
  {/if}
</div>

<style>
  .milestones-page {
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

</style>
