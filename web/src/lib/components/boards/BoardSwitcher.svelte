<script lang="ts">
  import type { Board } from '$lib/types/entities';

  let {
    boards,
    activeBoardId,
    onSelect,
    onAddBoard,
  }: {
    boards: Board[];
    activeBoardId: number | null;
    onSelect: (id: number) => void;
    onAddBoard: () => void;
  } = $props();
</script>

<div class="board-tabs">
  {#each boards as b (b.id)}
    <button
      class="board-tab"
      class:active={b.id === activeBoardId}
      onclick={() => onSelect(b.id)}
    >{b.name}</button>
  {/each}
  <button class="btn-ghost btn-sm" onclick={onAddBoard}>+ Board</button>
</div>

<style>
  .board-tabs { display: flex; gap: 4px; align-items: center; flex-wrap: wrap; }
  .board-tab {
    padding: 5px 12px; border: 1px solid var(--border);
    border-radius: var(--radius); background: none; color: var(--text-primary);
    font-size: 13px; cursor: pointer;
  }
  .board-tab:hover { background: var(--bg-secondary); }
  .board-tab.active { background: var(--accent); color: #fff; border-color: var(--accent); }

  .btn-ghost {
    padding: 5px 10px; background: none; border: none;
    color: var(--text-secondary); font-size: 13px; cursor: pointer; border-radius: var(--radius);
  }
  .btn-ghost:hover { background: var(--bg-secondary); color: var(--text-primary); }
  .btn-sm { padding: 4px 10px; font-size: 12px; }
</style>
