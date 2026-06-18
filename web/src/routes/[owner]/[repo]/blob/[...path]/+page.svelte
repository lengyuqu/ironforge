<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { repos } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { onMount } from 'svelte';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let filePath = $derived($page.params.path!);
  let blobData = $state<any>(null);
  let ref = $state('');
  let loading = $state(true);
  let error = $state('');
  let isMarkdown = $derived(filePath?.endsWith('.md') || filePath?.endsWith('.markdown'));

  // Helper function to get language class
  function getLangClass(): string {
    if (!blobData) return '';
    const ext = filePath.split('.').pop()?.toLowerCase();
    const map: Record<string, string> = {
      c: 'c', h: 'c', cpp: 'cpp', cc: 'cpp', cxx: 'cpp', hpp: 'cpp',
      rs: 'rust', go: 'go', py: 'python', js: 'javascript', ts: 'typescript',
      jsx: 'javascript', tsx: 'typescript', java: 'java', rb: 'ruby',
      php: 'php', swift: 'swift', kt: 'kotlin', dart: 'dart',
      html: 'html', css: 'css', scss: 'scss', json: 'json', xml: 'xml',
      yaml: 'yaml', yml: 'yaml', toml: 'ini', md: 'markdown',
      sh: 'bash', bash: 'bash', zsh: 'bash', sql: 'sql',
      dockerfile: 'dockerfile', makefile: 'makefile',
    };
    return map[ext || ''] || '';
  }
  
  let langClass = $derived(getLangClass());

  // Helper function to build breadcrumb path
  function getBreadcrumbs(): { name: string; href: string }[] {
    const parts = filePath.split('/');
    const crumbs: { name: string; href: string }[] = [];
    let accumulated = '';
    for (let i = 0; i < parts.length - 1; i++) {
      accumulated = accumulated ? accumulated + '/' + parts[i] : parts[i];
      crumbs.push({ name: parts[i], href: `/${owner}/${repo}/tree/${accumulated}` });
    }
    return crumbs;
  }

  let breadcrumbs = $derived(getBreadcrumbs());

  $effect(() => {
    loadBlob();
  });

  async function loadBlob() {
    loading = true;
    error = '';
    try {
      blobData = await repos.blob(owner, repo, filePath, ref || undefined);
      highlightCode();
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function highlightCode() {
    // Try to use highlight.js dynamically
    try {
      import('highlight.js').then(hljs => {
        const blocks = document.querySelectorAll('pre code.hljs-code');
        blocks.forEach(block => {
          hljs.default.highlightElement(block as HTMLElement);
        });
      }).catch(() => {});
    } catch { /* highlight.js not available */ }
  }

  function formatFileSize(size: number) {
    if (size < 1024) return size + t('repo.file_size.b');
    if (size < 1024 * 1024) return (size / 1024).toFixed(1) + t('repo.file_size.kb');
    return (size / (1024 * 1024)).toFixed(1) + t('repo.file_size.mb');
  }

  function getLineNumbers(content: string): { num: number; text: string }[] {
    return content.split('\n').map((line, i) => ({ num: i + 1, text: line }));
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
    <!-- Breadcrumb -->
    <div class="blob-breadcrumb">
      <a href="/{owner}/{repo}" class="crumb-link">{repo}</a>
      {#each breadcrumbs as crumb}
        <span class="crumb-sep">/</span>
        <a href={crumb.href} class="crumb-link">{crumb.name}</a>
      {/each}
    </div>

    <!-- File header -->
    <div class="file-header">
      <div class="file-info">
        <span class="file-path">
          <span class="file-icon">📄</span>
          {filePath.split('/').pop()}
        </span>
      </div>
      <div class="file-meta">
        <span class="file-lines">{getLineNumbers(blobData.content).length} lines</span>
        <span class="file-size">({formatFileSize(blobData.size)})</span>
      </div>
    </div>

    <!-- File content -->
    <div class="file-content">
      {#if isMarkdown}
        <div class="markdown-body">
          {@html blobData.content}
        </div>
      {:else}
        <div class="code-view">
          <table class="code-table">
            <tbody>
              {#each getLineNumbers(blobData.content) as line}
                <tr>
                  <td class="line-number">{line.num}</td>
                  <td class="line-content">
                    <code class="hljs-code {langClass}">{line.text || ' '}</code>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
/* Breadcrumb */
  .blob-breadcrumb {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 14px;
    margin-bottom: 12px;
  }

  .crumb-link {
    color: var(--accent);
    text-decoration: none;
  }
  .crumb-link:hover { text-decoration: underline; }

  .crumb-sep {
    color: var(--text-muted);
  }

  /* File header */
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

  .file-info {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .file-icon { font-size: 14px; }

  .file-path {
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
    font-weight: 600;
  }

  .file-meta {
    display: flex;
    align-items: center;
    gap: 12px;
    color: var(--text-muted);
    font-size: 12px;
  }

  .file-lines {
    color: var(--text-secondary);
  }

  /* Code view with line numbers */
  .file-content {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 0 0 var(--radius) var(--radius);
    overflow: auto;
  }

  .code-view {
    overflow-x: auto;
  }

  .code-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
    line-height: 1.6;
  }

  .code-table tr:hover {
    background: rgba(128, 128, 128, 0.05);
  }

  .line-number {
    padding: 0 12px 0 16px;
    text-align: right;
    color: var(--text-muted);
    font-size: 12px;
    user-select: none;
    border-right: 1px solid var(--border-light);
    white-space: nowrap;
    vertical-align: top;
    width: 1%;
  }

  .line-content {
    padding: 0 16px;
    white-space: pre;
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
  }

  .hljs-code {
    font-family: inherit;
    font-size: inherit;
    line-height: inherit;
    white-space: pre;
    tab-size: 4;
    -moz-tab-size: 4;
  }

  /* Markdown */
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
