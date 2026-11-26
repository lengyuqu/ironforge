<script lang="ts">
  // Milestones settings page — orchestration layer: loads the milestone
  // list and manages modal visibility. Create/edit/delete are handled by
  // the self-contained modal components, which reload the list on success.
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { page } from '$app/state';
  import { milestones } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { createLatest } from '$lib/utils/latest';
  import { toErrorMessage } from '$lib/utils/error';
  import type { Milestone } from '$lib/types/entities';
  import MilestoneFormModal from '$lib/components/milestones/MilestoneFormModal.svelte';
  import MilestoneDeleteModal from '$lib/components/milestones/MilestoneDeleteModal.svelte';
  import MilestoneGrid from '$lib/components/milestones/MilestoneGrid.svelte';

  let { data } = $props();
  const t = createT();
  const latest = createLatest();

  const owner = $derived(page.params.owner!);
  const repo = $derived(page.params.repo!);

  let milestoneList = $state<Milestone[]>([]);
  let loading = $state(true);
  let error = $state('');

  // Modal state
  let showForm = $state(false);
  let editingMilestone = $state<Milestone | null>(null);
  let deletingMilestone = $state<Milestone | null>(null);

  $effect(() => {
    void loadMilestones();
  });

  async function loadMilestones() {
    const reqOwner = owner;
    const reqRepo = repo;
    const isCurrent = latest.begin();
    try {
      loading = true;
      error = '';
      const list = await milestones.list(reqOwner, reqRepo);
      if (!isCurrent()) return;
      milestoneList = list;
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

<svelte:head>
  <title>{t('settings.milestones')} · {owner}/{repo} · IronForge</title>
</svelte:head>

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

  h1 {
    color: var(--text-primary);
    margin: 0;
  }

</style>
