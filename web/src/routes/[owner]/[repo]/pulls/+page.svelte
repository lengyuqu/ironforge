<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { pulls } from '$lib/api/client.svelte';
  import type { PullRequest } from '$lib/types/entities';
  import PullFilterTabs from '$lib/components/pulls/PullFilterTabs.svelte';
  import PullList from '$lib/components/pulls/PullList.svelte';
  import PullCreateForm from '$lib/components/pulls/PullCreateForm.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  let prList = $state<PullRequest[]>([]);
  let loading = $state(true);
  let error = $state('');
  let filterState = $state('open');
  let showCreate = $state(false);

  $effect(() => {
    loadPRs();
  });

  async function loadPRs() {
    try {
      loading = true;
      error = '';
      prList = (await pulls.list(owner, repo, filterState)).data;
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function handleCreated() {
    showCreate = false;
    await loadPRs();
  }
</script>

<svelte:head>
  <title>Pull Requests · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="pulls" starsCount={0} />

  <div class="gh-toolbar pulls-toolbar">
    <PullFilterTabs
      filter={filterState}
      onFilterChange={(next) => {
        filterState = next;
        loadPRs();
      }}
    />
    <button class="btn-primary" onclick={() => (showCreate = !showCreate)}>
      {t('pulls.new')}
    </button>
  </div>

  {#if showCreate}
    <PullCreateForm {owner} {repo} onCreated={handleCreated} onCancel={() => (showCreate = false)} />
  {/if}

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <PullList {owner} {repo} pullRequests={prList} {loading} filter={filterState} />
</div>

<style>
  .pulls-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  .btn-primary {
    padding: 6px 16px;
    background: var(--accent);
    color: #fff;
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }

  .btn-primary:hover {
    background: var(--accent-hover);
  }

  .error-banner {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 10px 12px;
    border-radius: var(--radius);
    margin-bottom: 16px;
  }
</style>
