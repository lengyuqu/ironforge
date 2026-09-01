<script lang="ts">
  // README section — renders the repo-root README (if any) as markdown.
  // Pure presentation; markdown rendering happens here via renderMarkdown.
  import { createT } from '$lib/i18n';
  import { renderMarkdown } from '$lib/utils/markdown';

  interface Props {
    content: string | null;
    loading?: boolean;
  }

  let { content, loading = false }: Props = $props();

  const t = createT();
</script>

{#if content}
  <div class="gh-card readme-section">
    <div class="readme-header">
      <span>📄 README.md</span>
    </div>
    <div class="readme-body">
      <div class="markdown-body">
        {@html renderMarkdown(content)}
      </div>
    </div>
  </div>
{:else if loading}
  <div class="gh-card readme-section">
    <p class="text-secondary">{t('common.loading')}</p>
  </div>
{/if}

<style>
  .readme-section {
    margin-top: 24px;
    padding: 0;
  }

  .readme-header {
    padding: 10px 16px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-bottom: none;
    border-radius: var(--radius) var(--radius) 0 0;
    font-size: 13px;
    font-weight: 600;
  }

  .readme-body {
    background: var(--bg-secondary);
    border: none;
    border-radius: 0;
    padding: 32px;
    max-height: 80vh;
    overflow-y: auto;
  }

  .markdown-body {
    line-height: 1.7;
    color: var(--text-primary);
  }

  .markdown-body :global(code) {
    background: var(--bg-tertiary);
    padding: 1px 6px;
    border-radius: 3px;
    font-size: 13px;
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
  }

  .markdown-body :global(a) {
    color: var(--accent);
    text-decoration: none;
  }
  .markdown-body :global(a:hover) { text-decoration: underline; }

  .markdown-body :global(strong) { font-weight: 700; }
</style>
