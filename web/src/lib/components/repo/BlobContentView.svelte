<script lang="ts">
  // Blob content view — renders the file body: markdown (rendered/source),
  // code with line numbers + selective highlight.js, binary or empty placeholders.
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { createT } from '$lib/i18n';
  import { languageFromPath } from '$lib/utils/editorFile';
  import { highlightCodeToLines } from '$lib/utils/highlight';
  import { renderMarkdown } from '$lib/utils/markdown';
  import type { BlobContent } from '$lib/types/entities';

  interface Props {
    blob: BlobContent;
    filePath: string;
    isText: boolean;
    isMarkdown: boolean;
    viewMode: 'rendered' | 'source';
  }

  let { blob, filePath, isText, isMarkdown, viewMode }: Props = $props();

  const t = createT();

  const contentLines = $derived(
    isText && blob ? blob.content.split('\n').map((line, i) => ({ num: i + 1, text: line })) : []
  );

  const renderedMarkdown = $derived(
    isMarkdown && isText && blob?.content ? renderMarkdown(blob.content) : ''
  );

  const lang = $derived(languageFromPath(filePath));
  let highlightedLines = $state<string[] | null>(null);
  let highlightRequestId = 0;

  $effect(() => {
    const content = blob?.content;
    const activeLang = lang;
    const shouldHighlight = Boolean(
      blob && isText && !blob.is_binary && content && (!isMarkdown || viewMode === 'source')
    );

    if (!shouldHighlight || !content) {
      highlightedLines = null;
      return;
    }

    const reqId = ++highlightRequestId;
    highlightedLines = null;
    highlightCodeToLines(content, activeLang).then((lines) => {
      if (reqId === highlightRequestId) {
        highlightedLines = lines;
      }
    });
  });
</script>

<div class="file-content">
  {#if blob.is_binary}
    <EmptyState title={t('repo.blob.binary_file')} />
  {:else if isMarkdown && viewMode === 'rendered'}
    <div class="markdown-body">
      {@html renderedMarkdown || `<p>${t('repo.blob.empty')}</p>`}
    </div>
  {:else if blob.content}
    <div class="code-view">
      <table class="code-table">
        <tbody>
          {#each contentLines as line, index (line.num)}
            <tr>
              <td class="line-number">{line.num}</td>
              <td class="line-content">
                <code class="hljs hljs-code {lang ? `language-${lang}` : ''}">{#if highlightedLines && highlightedLines[index] !== undefined}{@html highlightedLines[index] || ' '}{:else}{line.text || ' '}{/if}</code>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <EmptyState title={t('repo.blob.empty')} />
  {/if}
</div>

<style>
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
    font-family: var(--font-mono);
  }

  .hljs-code {
    font-family: inherit;
    font-size: inherit;
    line-height: inherit;
    white-space: pre;
    tab-size: 4;
  }

  .markdown-body {
    padding: 32px;
    line-height: 1.7;
  }

  .markdown-body :global(h1),
  .markdown-body :global(h2),
  .markdown-body :global(h3) {
    margin-top: 24px;
    margin-bottom: 12px;
    border-bottom: 1px solid var(--border);
    padding-bottom: 8px;
  }

  .markdown-body :global(p) {
    margin-bottom: 12px;
  }

  .markdown-body :global(code) {
    background: var(--bg-tertiary);
    padding: 2px 6px;
    border-radius: 3px;
    font-size: 12px;
  }

  .markdown-body :global(pre code) {
    display: block;
    padding: 12px;
    overflow: auto;
  }
</style>
