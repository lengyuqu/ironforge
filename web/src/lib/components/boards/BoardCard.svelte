<script lang="ts">
  import type { BoardCard as BoardCardType } from '$lib/types/entities';

  let {
    owner,
    repo,
    card,
    onDragStart,
    onDelete,
  }: {
    owner: string;
    repo: string;
    card: BoardCardType;
    onDragStart: (cardId: number, e: DragEvent) => void;
    onDelete: (cardId: number) => void;
  } = $props();

  function handleDragStart(e: DragEvent) {
    // Card id travels with the drag payload; drop targets read it via dataTransfer.
    e.dataTransfer?.setData('text/plain', String(card.id));
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
    onDragStart(card.id, e);
  }
</script>

<div
  class="card"
  draggable="true"
  ondragstart={handleDragStart}
  role="button"
  tabindex="0"
>
  <div class="card-content">
    {#if card.issue}
      <a href={`/${owner}/${repo}/issues/${card.issue.number}`} class="card-issue-link">
        #{card.issue.number}
      </a>
    {/if}
    <span class="card-note">{card.note || ''}</span>
  </div>
  <button
    class="card-delete"
    onclick={() => onDelete(card.id)}
    title="Remove card"
  >✕</button>
</div>

<style>
  .card {
    background: var(--bg-primary); border: 1px solid var(--border);
    border-radius: var(--radius); padding: 10px 10px 10px 12px;
    cursor: grab; transition: border-color 0.15s, box-shadow 0.15s;
    display: flex; align-items: flex-start; justify-content: space-between; gap: 8px;
  }
  .card:hover { border-color: var(--text-muted); box-shadow: 0 1px 4px rgba(0,0,0,0.15); }
  .card-content { flex: 1; min-width: 0; }
  .card-issue-link {
    font-size: 11px; color: var(--accent); text-decoration: none; font-weight: 600;
    display: block; margin-bottom: 2px;
  }
  .card-note { font-size: 13px; color: var(--text-primary); word-break: break-word; }
  .card-delete {
    flex: 0 0 auto; background: none; border: none; color: var(--text-muted);
    font-size: 12px; cursor: pointer; padding: 0; line-height: 1;
    opacity: 0; transition: opacity 0.1s;
  }
  .card:hover .card-delete { opacity: 1; }
  .card-delete:hover { color: var(--red); }
</style>
