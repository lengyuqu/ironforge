<script lang="ts">
  import { createT } from '$lib/i18n';
  import { renderMarkdown } from '$lib/utils/markdown';

  interface Props {
    content: string;
    isMarkdown: boolean;
  }

  let { content, isMarkdown }: Props = $props();

  const t = createT();

  let rendered = $derived(isMarkdown ? renderMarkdown(content) : '');
</script>

<div class="preview-pane">
  {#if isMarkdown}
    <div class="markdown-body">
      {@html rendered || `<p>${t('repo.blob.empty')}</p>`}
    </div>
  {:else if content}
    <pre><code>{content}</code></pre>
  {:else}
    <p class="muted">{t('repo.blob.empty')}</p>
  {/if}
</div>

<style>
  .preview-pane {
    min-height: 360px;
    max-height: 680px;
    overflow: auto;
    background: var(--bg-primary);
  }

  .preview-pane pre {
    margin: 0;
    padding: 16px;
    white-space: pre-wrap;
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
    font-size: 13px;
    line-height: 1.55;
  }

  .markdown-body {
    padding: 24px;
    line-height: 1.7;
  }

  .markdown-body :global(h1),
  .markdown-body :global(h2),
  .markdown-body :global(h3) {
    margin-top: 22px;
    margin-bottom: 10px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border);
  }

  .markdown-body :global(p) {
    margin-bottom: 12px;
  }

  .markdown-body :global(code) {
    background: var(--bg-tertiary);
    padding: 2px 5px;
    border-radius: 3px;
    font-size: 12px;
  }

  .markdown-body :global(pre code) {
    display: block;
    padding: 12px;
    overflow: auto;
  }

  .muted {
    margin: 0;
    padding: 16px;
    color: var(--text-muted);
  }
</style>
