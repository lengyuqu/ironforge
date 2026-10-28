<script lang="ts">
  // Quick-pick buttons for tags that already exist in the repository.
  import { createT } from '$lib/i18n';

  const MAX_HINTS = 10;

  interface Props {
    tags: string[];
    onPick: (tag: string) => void;
  }

  let { tags, onPick }: Props = $props();

  const t = createT();
</script>

{#if tags.length > 0}
  <div class="tag-hints">
    <span class="hint-label">{t('releases.existing_tags')}</span>
    {#each tags.slice(0, MAX_HINTS) as tag}
      <button type="button" class="tag-hint" onclick={() => onPick(tag)}>
        {tag}
      </button>
    {/each}
    {#if tags.length > MAX_HINTS}
      <span class="hint-more">{t('releases.more_tags', { count: tags.length - MAX_HINTS })}</span>
    {/if}
  </div>
{/if}

<style>
  .tag-hints {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-top: 8px;
  }

  .hint-label {
    font-size: 12px;
    color: var(--text-muted);
  }

  .tag-hint {
    padding: 2px 8px;
    font-size: 12px;
    color: var(--text-secondary);
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    border-radius: 4px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .tag-hint:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .hint-more {
    font-size: 12px;
    color: var(--text-muted);
  }
</style>
