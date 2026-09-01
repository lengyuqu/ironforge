<script lang="ts">
  // Repo overview page — orchestration layer: loads tree/branches/commits/
  // repo info in parallel, keeps the URL in sync with ref/path, and renders
  // the browser components. README detection probes common filenames.
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { repos } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { buildRepoQuery } from '$lib/utils/repoUrls';
  import type { Branch, RepoCommitEntry, RepoInfo, RepoTreeEntry } from '$lib/types/entities';
  import EmptyRepoGuide from '$lib/components/repo/EmptyRepoGuide.svelte';
  import RepoToolbar from '$lib/components/repo/RepoToolbar.svelte';
  import FileTreePanel from '$lib/components/repo/FileTreePanel.svelte';
  import RecentCommitsPanel from '$lib/components/repo/RecentCommitsPanel.svelte';
  import ReadmeSection from '$lib/components/repo/ReadmeSection.svelte';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let ref = $state('');
  let path = $state('');
  let queryRef = $derived($page.url.searchParams.get('ref') || '');
  let queryPath = $derived($page.url.searchParams.get('path') || '');
  let entries = $state<RepoTreeEntry[]>([]);
  let branches = $state<Branch[]>([]);
  let commits = $state<RepoCommitEntry[]>([]);
  let repoInfo = $state<RepoInfo | null>(null);
  let readmeContent = $state<string | null>(null);
  let readmeLoading = $state(false);
  let loading = $state(true);
  let error = $state('');
  let currentRefLabel = $derived(
    ref || repoInfo?.default_branch || branches.find((b) => b.is_default)?.name || 'main'
  );

  function syncLocation(nextRef = ref, nextPath = path) {
    const normalizedPath = nextPath ? nextPath.replace(/\/+/g, '/') : '';
    ref = nextRef;
    path = normalizedPath;
    goto(`/${owner}/${repo}${buildRepoQuery(nextRef, normalizedPath)}`, { replaceState: true });
  }

  $effect(() => {
    if (ref !== queryRef || path !== queryPath) {
      ref = queryRef;
      path = queryPath;
    }
    loadData();
  });

  async function loadData() {
    loading = true;
    error = '';
    readmeContent = null;
    try {
      const [treeData, branchData, logData, repoData] = await Promise.all([
        repos.tree(owner, repo, ref || undefined, path || undefined),
        repos.branches(owner, repo),
        repos.log(owner, repo, ref || undefined, path || undefined),
        repos.get(owner, repo),
      ]);
      entries = treeData.entries || [];
      branches = branchData || [];
      commits = (logData.commits || []).slice(0, 5);
      repoInfo = repoData;

      // Load README when at root
      if (!path) {
        loadReadme();
      }
    } catch (e: unknown) {
      error = toErrorMessage(e, t('errors.load_failed', 'Load failed'));
    } finally {
      loading = false;
    }
  }

  async function loadReadme() {
    readmeLoading = true;
    try {
      // Try common README filenames
      const readmeNames = ['README.md', 'README.markdown', 'README', 'readme.md', 'Readme.md'];
      for (const name of readmeNames) {
        const entry = entries.find((e) => e.name === name);
        if (entry) {
          const data = await repos.blob(owner, repo, name, ref || undefined);
          readmeContent = data.content;
          break;
        }
      }
    } catch {
      // No README found — that's OK
    } finally {
      readmeLoading = false;
    }
  }

  function navigateToPath(entryName: string) {
    const nextPath = path ? `${path}/${entryName}` : entryName;
    syncLocation(ref, nextPath);
  }

  function navigateUp() {
    const parts = path.split('/');
    parts.pop();
    syncLocation(ref, parts.join('/'));
  }

  function selectBranch(branchName: string) {
    syncLocation(branchName, path);
  }
</script>

<svelte:head>
  <title>{owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="code" starsCount={repoInfo?.stars_count || 0} defaultBranch={repoInfo?.default_branch} />

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">{t('common.loading')}</p>
  {:else if commits.length === 0 && entries.length === 0}
    <!-- Empty repository — setup guidance -->
    <EmptyRepoGuide {owner} {repo} defaultBranch={repoInfo?.default_branch || 'main'} />
  {:else}
    <RepoToolbar
      {owner}
      {repo}
      {ref}
      {path}
      {branches}
      {currentRefLabel}
      onSelectBranch={selectBranch}
    />

    <div class="content-grid">
      <FileTreePanel
        {owner}
        {repo}
        {ref}
        {path}
        {entries}
        onNavigateTo={navigateToPath}
        onNavigateUp={navigateUp}
      />
      <RecentCommitsPanel {owner} {repo} {ref} {commits} />
    </div>

    <!-- README rendering (at repo root) -->
    {#if !path}
      <ReadmeSection content={readmeContent} loading={readmeLoading} />
    {/if}
  {/if}
</div>

<style>
  .content-grid {
    display: grid;
    grid-template-columns: 1fr 320px;
    gap: 16px;
  }

  @media (max-width: 900px) {
    .content-grid { grid-template-columns: 1fr; }
  }
</style>
