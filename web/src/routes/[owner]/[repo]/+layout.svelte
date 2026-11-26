<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { repos } from '$lib/api/client.svelte';
  import { setRepoContext } from '$lib/stores/repoContext.svelte';
  import type { RepoInfo } from '$lib/types/entities';
  import type { Snippet } from 'svelte';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  let repoInfo = $state<RepoInfo | null>(null);
  let loadedScope = '';
  let loadVersion = 0;

  // Derive the active tab and whether the page uses the full-bleed file editor.
  let subpath = $derived.by(() => {
    const segments = $page.url.pathname.split('/').filter(Boolean);
    return segments.slice(2).join('/');
  });

  let isEditorRoute = $derived(subpath === 'new' || subpath.startsWith('edit/'));

  let activeTab = $derived.by(() => {
    if (subpath === 'boards' || subpath === 'issues/board' || subpath.startsWith('issues/board/')) {
      return 'board';
    }
    if (subpath === 'issues' || subpath.startsWith('issues/')) return 'issues';
    if (subpath === 'pulls' || subpath.startsWith('pulls/')) return 'pulls';
    if (subpath === 'wiki' || subpath.startsWith('wiki/')) return 'wiki';
    if (subpath === 'pipelines' || subpath.startsWith('pipelines/')) return 'pipelines';
    if (subpath === 'releases' || subpath.startsWith('releases/')) return 'releases';
    if (subpath === 'packages' || subpath.startsWith('packages/')) return 'packages';
    if (subpath === 'time_tracking' || subpath.startsWith('time_tracking/')) return 'time_tracking';
    if (subpath === 'collaboration' || subpath.startsWith('collaboration/')) return 'collaboration';
    if (subpath === 'commits' || subpath.startsWith('commits/')) return 'commits';
    if (subpath === 'settings' || subpath.startsWith('settings/')) return 'settings';
    return 'code';
  });

  async function refresh() {
    const scope = `${owner}/${repo}`;
    const ticket = ++loadVersion;
    try {
      const info = await repos.get(owner, repo);
      if (ticket === loadVersion && scope === `${owner}/${repo}`) {
        repoInfo = info;
      }
    } catch {
      // Child pages render dedicated error/not-found states when repo access fails.
    }
  }

  setRepoContext({
    get repoInfo() {
      return repoInfo;
    },
    refresh,
    setRepoInfo(info) {
      repoInfo = info;
    },
  });

  $effect(() => {
    const scope = `${owner}/${repo}`;
    if (scope === loadedScope) return;
    loadedScope = scope;
    repoInfo = null;
    void refresh();
  });
</script>

{#if isEditorRoute}
  {@render children()}
{:else}
  <div class="page-container">
    <RepoHeader
      {owner}
      {repo}
      {activeTab}
      starsCount={repoInfo?.stars_count || 0}
      defaultBranch={repoInfo?.default_branch}
    />
    {@render children()}
  </div>
{/if}
