<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
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
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      loading = false;
    }
  }

  function handleCreated() {
    goto(`/${owner}/${repo}/releases`);
  }
</script>

<svelte:head>
  <title>{t('releases.new')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader owner={owner!} repo={repo!} activeTab="releases" />

  <div class="page-header">
    <h1>{t('releases.create_title')}</h1>
  </div>

  {#if error}
    <ErrorBanner message={error} />
  {/if}

  {#if loading}
    <LoadingState />
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

