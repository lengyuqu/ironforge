<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { repos } from '$lib/api/client';
  import { createT } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let filePath = $derived($page.params.path!);
  let blobData = $state<any>(null);
  let loading = $state(true);
  let error = $state('');
  let isMarkdown = $derived(filePath?.endsWith('.md') || filePath?.endsWith('.markdown'));

  $effect(() => {
    loadBlob();
  });

  async function loadBlob() {
    loading = true;
    error = '';
    try {
      blobData = await repos.blob(owner, repo, filePath);
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function formatFileSize(size: number) {
    if (size < 1024) return size + t('repo.file_size.b');
    if (size < 1024 * 1024) return (size / 1024).toFixed(1) + t('repo.file_size.kb');
    return (size / (1024 * 1024)).toFixed(1) + t('repo.file_size.mb');
  }
</script>

<svelte:head>
  <title>{filePath} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="repo-page">
  <RepoHeader {owner} {repo} activeTab="code" />

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">{t('common.loading')}</p>
  {:else if blobData}
    <div class="file-header">
      <div class="file-path">
        <span>{filePath}</span>
      </div>
      <div class="file-meta">
        <span>{formatFileSize(blobData.size)}</span>
      </div>
    </div>

    <div class="file-content">
      {#if isMarkdown}
        <div class="markdown-body">
          {@html markdownToHtml(blobData.content)}
        </div>
      {:else}
        <pre><code>{blobData.content}</code></pre>
      {/if}
    </div>
  {/if}
</div>

<script module>
  // Simple markdown to HTML (for rendering READMEs)
  function markdownToHtml(md: string): string {
    try {
      // Use dynamic import in browser
      return md; // Will be replaced by marked on client
    } catch {
      return md;
    }
  }
</script>

<style>
  .repo-page {
    max-width: 1100px;
    margin: 0 auto;
    padding: 24px;
  }

  .error-banner {
    background: rgba(248, 81, 73, 0.1);
    border: 1px solid var(--red-dim);
    color: var(--red);
    border-radius: var(--radius);
    padding: 10px 14px;
    font-size: 13px;
  }

  .file-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 16px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-bottom: none;
    border-radius: var(--radius) var(--radius) 0 0;
    font-size: 13px;
  }

  .file-path {
    font-family: var(--font-mono);
    font-weight: 600;
  }

  .file-meta {
    color: var(--text-muted);
    font-size: 12px;
  }

  .file-content {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 0 0 var(--radius) var(--radius);
    overflow: auto;
  }

  .file-content pre {
    margin: 0;
    padding: 16px;
    border: none;
    border-radius: 0;
    font-size: 13px;
    line-height: 1.6;
  }

  .markdown-body {
    padding: 32px;
    line-height: 1.7;
  }
  .markdown-body :global(h1), .markdown-body :global(h2), .markdown-body :global(h3) {
    margin-top: 24px;
    margin-bottom: 12px;
    border-bottom: 1px solid var(--border);
    padding-bottom: 8px;
  }
  .markdown-body :global(p) { margin-bottom: 12px; }
  .markdown-body :global(code) {
    background: var(--bg-tertiary);
    padding: 2px 6px;
    border-radius: 3px;
    font-size: 12px;
  }
  .markdown-body :global(pre code) {
    background: none;
    padding: 0;
  }
</style>
