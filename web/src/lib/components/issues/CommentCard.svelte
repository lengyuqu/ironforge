<script lang="ts">
  // Comment card — shared by the issue body and each comment: header (author
  // + date), markdown body, reaction bar, and an optional children snippet
  // for extras such as the attachment panel. Pure presentation.
  import type { Snippet } from 'svelte';
  import { createT, formatDate } from '$lib/i18n';
  import { renderMarkdown as renderMarkdownSafe } from '$lib/utils/markdown';
  import type { ReactionSummary } from '$lib/api/client.svelte';
  import ReactionBar from './ReactionBar.svelte';

  interface Props {
    author?: string | null;
    createdAt?: string;
    body?: string | null;
    reactions: ReactionSummary[];
    onToggleReaction: (content: string) => void;
    children?: Snippet;
  }

  let { author, createdAt, body, reactions, onToggleReaction, children }: Props = $props();

  const t = createT();

  function renderMarkdown(content: string | null | undefined): string {
    if (!content) return '';
    return renderMarkdownSafe(content);
  }
</script>

<div class="card">
  <div class="card-header">
    {t('issues.commented', { author: author || t('common.unknown'), date: formatDate(createdAt || '') })}
  </div>
  <div class="card-body markdown-body">{@html renderMarkdown(body)}</div>
  <ReactionBar rows={reactions} onToggle={onToggleReaction} />
  {@render children?.()}
</div>

<style>
  .card {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    margin-bottom: 12px;
  }

  .card-header {
    padding: 8px 16px;
    background: var(--bg-tertiary);
    font-size: 13px;
    color: var(--text-secondary);
  }

  .card-body {
    padding: 16px;
    font-size: 14px;
    line-height: 1.6;
  }

  .card-body :global(p) {
    margin: 0 0 12px;
  }

  .card-body :global(p:last-child) {
    margin-bottom: 0;
  }

  .card-body :global(ul),
  .card-body :global(ol) {
    padding-left: 24px;
    margin: 8px 0 12px;
  }

  .card-body :global(pre) {
    overflow-x: auto;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px;
  }

  .card-body :global(code) {
    font-family: var(--font-mono);
    font-size: 12px;
  }
</style>
