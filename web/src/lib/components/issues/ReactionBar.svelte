<script lang="ts">
  // Reaction bar — pure presentation: renders the fixed emoji set with counts
  // and my-reaction state, delegating toggles to the parent.
  import { createT } from '$lib/i18n';
  import type { ReactionSummary } from '$lib/api/client.svelte';

  const REACTION_EMOJI: Record<string, string> = {
    '+1': '👍',
    '-1': '👎',
    laugh: '😄',
    confused: '😕',
    heart: '❤️',
    hooray: '🎉',
    rocket: '🚀',
    eyes: '👀',
  };

  interface Props {
    rows: ReactionSummary[];
    onToggle: (content: string) => void;
  }

  let { rows, onToggle }: Props = $props();

  const t = createT();
</script>

<div class="reaction-bar" role="group" aria-label={t('issues.reaction.title')}>
  {#each Object.entries(REACTION_EMOJI) as [content, emoji] (content)}
    {@const summary = rows.find((r) => r.content === content)}
    {@const count = summary?.count ?? 0}
    {@const mine = summary?.reacted_by_me ?? false}
    <button
      type="button"
      class="reaction-btn"
      class:mine
      title={t('issues.reaction.title')}
      aria-pressed={mine}
      onclick={() => onToggle(content)}
    >
      <span class="reaction-emoji">{emoji}</span>
      {#if count > 0}<span class="reaction-count">{count}</span>{/if}
    </button>
  {/each}
</div>

<style>
  .reaction-bar {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 0 16px 12px 16px;
    border-top: 1px solid var(--border);
    padding-top: 10px;
    margin-top: 4px;
  }

  .reaction-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    border: 1px solid var(--border);
    border-radius: 12px;
    font-size: 13px;
    cursor: pointer;
  }

  .reaction-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .reaction-btn.mine {
    border-color: var(--green);
    color: var(--green);
    background: rgba(63, 185, 80, 0.12);
  }

  .reaction-emoji {
    font-size: 14px;
    line-height: 1;
  }

  .reaction-count {
    font-size: 12px;
    font-weight: 600;
  }
</style>
