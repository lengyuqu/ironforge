<script lang="ts">
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { isAuthReady, isLoggedIn } from '$lib/stores/auth.svelte';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { repos } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import ReleaseForm from '$lib/components/releases/ReleaseForm.svelte';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  let branches = $state<string[]>([]);
  let tags = $state<string[]>([]);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    if (!isAuthReady()) return;
    if (!isLoggedIn()) {
      goto('/login');
      return;
    }
    loadBranchesAndTags();
  });

  async function loadBranchesAndTags() {
    loading = true;
    try {
      const [branchList, tagList] = await Promise.all([
        repos.branches(owner!, repo!),
        repos.tags(owner!, repo!)
      ]);
      branches = branchList.map(b => b.name);
      tags = tagList.map(t => t.name);
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function handleCreated() {
    goto(`/${owner}/${repo}/releases`);
  }
</script>

<svelte:head>
  <title>New Release · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader owner={owner!} repo={repo!} activeTab="releases" />

  <div class="page-header">
    <h1>{t('releases.create_title')}</h1>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="loading-text">{t('common.loading')}</p>
  {:else}
    <ReleaseForm
      owner={owner!}
      repo={repo!}
      {branches}
      {tags}
      onCreated={handleCreated}
    />
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

  .error-banner {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 10px 12px;
    border-radius: 6px;
    margin-bottom: 16px;
  }
</style>
