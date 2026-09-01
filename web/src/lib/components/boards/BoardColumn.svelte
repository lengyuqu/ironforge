<script lang="ts">
  import { boards } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import type { BoardCard as BoardCardType, BoardColumn as BoardColumnType } from '$lib/types/entities';
  import BoardCard from './BoardCard.svelte';

  let {
    owner,
    repo,
    boardId,
    column,
    cards,
    onRefresh,
  }: {
    owner: string;
    repo: string;
    boardId: number;
    column: BoardColumnType;
    cards: BoardCardType[];
    onRefresh: () => void | Promise<void>;
  } = $props();

  // Local drag highlight + add-card form state (was page-level Record keyed by column id)
  let dragOver = $state(false);
  let showAddCard = $state(false);
  let newCardNote = $state('');
  let cardIds = $state<number[]>([]); // card ids currently being dragged within this column

  function onDragOver(e: DragEvent) {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    dragOver = true;
  }

  function onDragLeave(e: DragEvent) {
    const rel = e.relatedTarget as Element | null;
    if (!rel || !(e.currentTarget as Element).contains(rel)) {
      dragOver = false;
    }
  }

  async function onDrop(e: DragEvent) {
    e.preventDefault();
    dragOver = false;
    const raw = e.dataTransfer?.getData('text/plain');
    const cardId = raw ? Number(raw) : NaN;
    if (!cardId || Number.isNaN(cardId)) return;

    // Same-column drop: no-op (matches original behaviour)
    if (cards.some((c) => c.id === cardId) || cardIds.includes(cardId)) {
      cardIds = [];
      return;
    }

    try {
      await boards.moveCard(owner, repo, boardId, cardId, {
        column_id: column.id,
        position: cards.length,
      });
      await onRefresh();
    } catch (err) {
      toast.error(toErrorMessage(err, 'Move failed'));
    } finally {
      cardIds = [];
    }
  }

  async function handleAddCard() {
    const note = newCardNote.trim();
    if (!note) return;
    try {
      await boards.createCard(owner, repo, boardId, column.id, { note });
      newCardNote = '';
      showAddCard = false;
      await onRefresh();
    } catch (err) {
      toast.error(toErrorMessage(err, 'Add card failed'));
    }
  }

  async function handleDeleteCard(cardId: number) {
    try {
      await boards.deleteCard(owner, repo, boardId, cardId);
      await onRefresh();
    } catch (err) {
      toast.error(toErrorMessage(err, 'Delete card failed'));
    }
  }

  async function handleDeleteColumn() {
    if (!confirm('Delete this column and all its cards?')) return;
    try {
      await boards.deleteColumn(owner, repo, boardId, column.id);
      await onRefresh();
    } catch (err) {
      toast.error(toErrorMessage(err, 'Delete column failed'));
    }
  }
</script>

<div
  class="board-column"
  class:drag-over={dragOver}
  ondragover={onDragOver}
  ondragleave={onDragLeave}
  ondrop={onDrop}
  role="list"
  aria-label={column.name}
>
  <div class="column-header" style="border-top: 3px solid {column.color || '#6366f1'}">
    <span class="column-name">{column.name}</span>
    <div class="column-actions">
      <span class="card-count">{cards.length}</span>
      <button class="btn-ghost btn-xs" onclick={handleDeleteColumn} title="Delete column">✕</button>
    </div>
  </div>

  <div class="column-body" role="listitem">
    {#each cards as card (card.id)}
      <BoardCard
        {owner}
        {repo}
        {card}
        onDragStart={(id) => {
          cardIds = [id];
        }}
        onDelete={handleDeleteCard}
      />
    {/each}

    {#if showAddCard}
      <div class="add-card-form">
        <textarea
          class="card-textarea"
          rows="2"
          placeholder="Add a note…"
          bind:value={newCardNote}
          onkeydown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleAddCard(); } }}
        ></textarea>
        <div class="add-card-actions">
          <button class="btn-primary btn-xs" onclick={handleAddCard}>Add</button>
          <button class="btn-ghost btn-xs" onclick={() => showAddCard = false}>Cancel</button>
        </div>
      </div>
    {:else}
      <button class="add-card-btn" onclick={() => showAddCard = true}>
        + Add card
      </button>
    {/if}
  </div>
</div>

<style>
  .board-column {
    flex: 0 0 280px; background: var(--bg-secondary);
    border: 1px solid var(--border); border-radius: var(--radius);
    display: flex; flex-direction: column; min-height: 200px;
    transition: background 0.15s;
  }
  .board-column.drag-over { background: var(--bg-hover); border-color: var(--accent); }

  .column-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 10px 12px; border-bottom: 1px solid var(--border);
  }
  .column-name { font-size: 13px; font-weight: 600; }
  .column-actions { display: flex; align-items: center; gap: 4px; }
  .card-count {
    background: var(--bg-tertiary); color: var(--text-muted);
    border-radius: 10px; font-size: 11px; font-weight: 600; padding: 1px 7px;
  }

  .column-body { padding: 8px; flex: 1; display: flex; flex-direction: column; gap: 8px; }

  .add-card-btn {
    display: block; width: 100%; padding: 7px; background: none; border: none;
    color: var(--text-muted); font-size: 13px; text-align: left; cursor: pointer;
    border-radius: var(--radius);
  }
  .add-card-btn:hover { background: var(--bg-hover); color: var(--text-primary); }

  .add-card-form { display: flex; flex-direction: column; gap: 6px; }
  .card-textarea {
    width: 100%; padding: 8px; border: 1px solid var(--border);
    border-radius: var(--radius); background: var(--bg-primary);
    color: var(--text-primary); font-size: 13px; resize: none; box-sizing: border-box;
  }
  .add-card-actions { display: flex; gap: 6px; }

  .btn-primary {
    padding: 6px 14px; background: var(--accent); color: #fff; border: none;
    border-radius: var(--radius); font-size: 13px; font-weight: 600; cursor: pointer;
  }
  .btn-primary:hover { filter: brightness(1.1); }
  .btn-ghost {
    padding: 5px 10px; background: none; border: none;
    color: var(--text-secondary); font-size: 13px; cursor: pointer; border-radius: var(--radius);
  }
  .btn-ghost:hover { background: var(--bg-secondary); color: var(--text-primary); }
  .btn-xs { padding: 3px 8px; font-size: 12px; }
</style>
