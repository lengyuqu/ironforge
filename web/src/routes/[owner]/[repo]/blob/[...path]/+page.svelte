<script lang="ts">
  // Blob viewer page — orchestration layer: loads the file content for the
  // current ref/path, keeps the URL ref in sync, and owns the view-mode and
  // delete-panel visibility state.
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { repos } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { buildTreeHref } from '$lib/utils/repoUrls';
  import type { BlobContent } from '$lib/types/entities';
  import BlobBreadcrumb from '$lib/components/repo/BlobBreadcrumb.svelte';
  import BlobFileHeader from '$lib/components/repo/BlobFileHeader.svelte';
  import BlobDeletePanel from '$lib/components/repo/BlobDeletePanel.svelte';
  import BlobContentView from '$lib/components/repo/BlobContentView.svelte';

  const t = createT();
  const MAX_EDITABLE_SIZE = 1024 * 1024;

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let filePath = $derived($page.params.path!);
  let queryRef = $derived($page.url.searchParams.get('ref') || '');

  let blobData = $state<BlobContent | null>(null);
  let ref = $state('');
  let loading = $state(true);
  let error = $state('');
  let loadKey = $state('');
  let viewMode = $state<'rendered' | 'source'>('rendered');
  let deleteOpen = $state(false);

  let isMarkdown = $derived(/\.(md|markdown)$/i.test(filePath));
  let isText = $derived(Boolean(blobData && !blobData.is_binary && blobData.encoding === 'utf-8'));
  let canEdit = $derived(Boolean(isText && blobData && blobData.size <= MAX_EDITABLE_SIZE));
  let lineCount = $derived(isText && blobData ? blobData.content.split('\n').length : 0);

  $effect(() => {
    const nextRef = queryRef;
    if (ref !== nextRef) ref = nextRef;

    const nextKey = `${owner}/${repo}/${filePath}/${nextRef}`;
    if (loadKey !== nextKey) {
      loadKey = nextKey;
      viewMode = 'rendered';
      deleteOpen = false;
      loadBlob(nextRef);
    }
  });

  async function loadBlob(activeRef: string) {
    loading = true;
    error = '';
    try {
      blobData = await repos.blob(owner, repo, filePath, activeRef || undefined);
    } catch (e: unknown) {
      error = toErrorMessage(e, t('errors.load_failed', 'Load failed'));
    } finally {
      loading = false;
    }
  }

  function handleDeleted() {
    goto(buildTreeHref(owner, repo, ref, ''));
  }
</script>

<svelte:head>
  <title>{filePath} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="code" />

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">{t('common.loading')}</p>
  {:else if blobData}
    <BlobBreadcrumb {owner} {repo} {ref} {filePath} />

    <BlobFileHeader
      {owner}
      {repo}
      {ref}
      {filePath}
      {isText}
      {isMarkdown}
      {lineCount}
      size={blobData.size}
      {canEdit}
      {viewMode}
      sha={blobData.sha}
      onToggleView={() => (viewMode = viewMode === 'rendered' ? 'source' : 'rendered')}
      onToggleDelete={() => (deleteOpen = !deleteOpen)}
    />

    {#if blobData.is_binary}
      <div class="warning-banner">{t('repo.blob.binary_file')}</div>
    {:else if blobData.size > MAX_EDITABLE_SIZE}
      <div class="warning-banner">{t('repo.blob.large_file')}</div>
    {/if}

    {#if deleteOpen && blobData.sha}
      <BlobDeletePanel
        {owner}
        {repo}
        {ref}
        {filePath}
        sha={blobData.sha}
        onClose={() => (deleteOpen = false)}
        onDeleted={handleDeleted}
      />
    {/if}

    <BlobContentView blob={blobData} {filePath} {isText} {isMarkdown} {viewMode} />
  {/if}
</div>

<style>
  .warning-banner {
    border: 1px solid color-mix(in srgb, #bf8700 35%, transparent);
    border-bottom: none;
    background: color-mix(in srgb, #bf8700 11%, transparent);
    padding: 10px 16px;
    font-size: 13px;
  }
</style>
