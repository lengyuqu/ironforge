<script lang="ts">
  // Shared suggestion block — renders the code suggestion attached to a review
  // comment (with optional "select for batch apply" checkbox and apply button).
  // Used by both the conversation threads view and the diff inline threads.
  import { createT } from '$lib/i18n';
  import type { ReviewComment } from '$lib/types/entities';

  interface Props {
    comment: ReviewComment;
    /** Show the batch-apply checkbox (only for applicable suggestions). */
    selectable?: boolean;
    selected?: boolean;
    onToggleSelect?: () => void;
    /** Apply this single suggestion; hidden once applied. */
    onApply?: () => void;
    applying?: boolean;
  }

  let {
    comment,
    selectable = false,
    selected = false,
    onToggleSelect,
    onApply,
    applying = false,
  }: Props = $props();

  const t = createT();
</script>

{#if comment.suggestion !== null && comment.suggestion !== undefined}
  <div class="suggestion-block">
    {#if selectable && onToggleSelect}
      <label class="suggestion-select">
        <input type="checkbox" checked={selected} onchange={onToggleSelect} />
        {t('pulls.suggestion.select')}
      </label>
    {/if}
    {#if comment.suggestion === ''}
      <em>{t('pulls.suggestion.delete_range')}</em>
    {:else}
      <code>{comment.suggestion}</code>
    {/if}
    {#if comment.suggestion_applied_at}
      <span>{t('pulls.suggestion.applied')}</span>
    {:else if onApply}
      <button class="btn-secondary" disabled={applying} onclick={onApply}>
        {t('pulls.suggestion.apply')}
      </button>
    {/if}
  </div>
{/if}

<style>
  .suggestion-block {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 8px 12px;
    padding: 10px;
    border: 1px solid var(--green-dim);
    border-radius: var(--radius);
    background: rgba(63, 185, 80, 0.08);
  }
  .suggestion-block code { white-space: pre-wrap; }
  .suggestion-block button { align-self: flex-start; }

  .suggestion-select {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .btn-secondary {
    padding: 6px 16px;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 14px;
    cursor: pointer;
  }
  .btn-secondary:hover:not(:disabled) { border-color: var(--accent); }
  .btn-secondary:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
