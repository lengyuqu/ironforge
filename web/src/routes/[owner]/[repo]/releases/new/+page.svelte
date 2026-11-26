<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { isAuthReady, isLoggedIn } from '$lib/stores/auth.svelte';
  import { repos } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { createLatest } from '$lib/utils/latest';
  import ReleaseForm from '$lib/components/releases/ReleaseForm.svelte';

  const t = createT();
  const latest = createLatest();

  let owner = $derived(page.params.owner!);
  let repo = $derived(page.params.repo!);

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
    void loadBranchesAndTags();
  });

  async function loadBranchesAndTags() {
    const reqOwner = owner!;
    const reqRepo = repo!;
    const isCurrent = latest.begin();
    loading = true;
    error = '';
    try {
      const [branchList, tagList] = await Promise.all([
        repos.branches(reqOwner, reqRepo),
        repos.tags(reqOwner, reqRepo)
      ]);
      if (!isCurrent()) return;
      branches = branchList.map(b => b.name);
      tags = tagList.map(t => t.name);
    } catch (e) {
      if (!isCurrent()) return;
      error = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      if (isCurrent()) {
        loading = false;
      }
    }
  }

  function handleCreated() {
    goto(`/${owner}/${repo}/releases`);
  }
</script>

<svelte:head>
  <title>{t('releases.new')} · {owner}/{repo} · IronForge</title>
</svelte:head>

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