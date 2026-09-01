<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { boards } from '$lib/api/client.svelte';
  import type { Board, BoardFull } from '$lib/types/entities';
  import BoardSwitcher from '$lib/components/boards/BoardSwitcher.svelte';
  import BoardCreateForm from '$lib/components/boards/BoardCreateForm.svelte';
  import ColumnCreateForm from '$lib/components/boards/ColumnCreateForm.svelte';
  import BoardColumn from '$lib/components/boards/BoardColumn.svelte';

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  let boardList = $state<Board[]>([]);
  let activeBoardId = $state<number | null>(null);
  let activeBoard = $state<BoardFull | null>(null);
  let loading = $state(true);
  let error = $state('');

  let showCreateBoard = $state(false);
  let showAddColumn = $state(false);

  $effect(() => { loadBoards(); });

  async function loadBoards() {
    loading = true;
    error = '';
    try {
      boardList = await boards.list(owner, repo);
      if (boardList.length > 0) {
        activeBoardId = activeBoardId ?? boardList[0].id;
        await loadBoard(activeBoardId!);
      }
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function loadBoard(id: number) {
    activeBoard = await boards.get(owner, repo, id);
  }

  async function handleSelectBoard(id: number) {
    activeBoardId = id;
    await loadBoard(id);
  }

  async function handleBoardCreated(id: number) {
    showCreateBoard = false;
    activeBoardId = id;
    await loadBoards();
  }

  async function handleColumnCreated() {
    showAddColumn = false;
    if (activeBoardId != null) await loadBoard(activeBoardId);
  }
</script>

<svelte:head>
  <title>Board · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="board" starsCount={0} />

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="loading-text">Loading…</p>
  {:else if boardList.length === 0 && !showCreateBoard}
    <!-- Empty state -->
    <div class="empty-state">
      <div class="empty-icon">📋</div>
      <h2>No boards yet</h2>
      <p>Create your first project board to organize issues.</p>
      <button class="btn-primary" onclick={() => showCreateBoard = true}>Create Board</button>
    </div>
  {:else}
    <!-- Board selector + controls -->
    <div class="board-toolbar">
      <BoardSwitcher
        boards={boardList}
        activeBoardId={activeBoardId}
        onSelect={handleSelectBoard}
        onAddBoard={() => showCreateBoard = !showCreateBoard}
      />
      <button class="btn-outline btn-sm" onclick={() => showAddColumn = !showAddColumn}>+ Column</button>
    </div>

    {#if showCreateBoard}
      <BoardCreateForm
        {owner}
        {repo}
        onCreated={handleBoardCreated}
        onCancel={() => showCreateBoard = false}
      />
    {/if}

    {#if showAddColumn && activeBoardId != null}
      <ColumnCreateForm
        {owner}
        {repo}
        boardId={activeBoardId}
        onCreated={handleColumnCreated}
        onCancel={() => showAddColumn = false}
      />
    {/if}

    <!-- Board columns -->
    {#if activeBoard?.columns}
      <div class="board-container">
        {#each activeBoard.columns as { column, cards } (column.id)}
          <BoardColumn
            {owner}
            {repo}
            boardId={activeBoard!.board.id}
            {column}
            {cards}
            onRefresh={async () => { if (activeBoardId != null) await loadBoard(activeBoardId); }}
          />
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .loading-text { color: var(--text-secondary); text-align: center; padding: 48px; }

  /* ── Empty state ── */
  .empty-state {
    text-align: center; padding: 80px 24px;
    display: flex; flex-direction: column; align-items: center; gap: 12px;
  }
  .empty-icon { font-size: 48px; }
  .empty-state h2 { font-size: 20px; font-weight: 600; margin: 0; }
  .empty-state p { color: var(--text-secondary); font-size: 14px; margin: 0; }

  /* ── Toolbar ── */
  .board-toolbar {
    display: flex; align-items: center; justify-content: space-between;
    margin-bottom: 16px; flex-wrap: wrap; gap: 8px;
  }

  /* ── Board layout ── */
  .board-container {
    display: flex; gap: 16px; overflow-x: auto; padding-bottom: 16px;
    align-items: flex-start;
  }

  .error-banner {
    color: #f85149; background: rgba(248, 81, 73, 0.1);
    padding: 10px 12px; border-radius: 6px; margin-bottom: 16px;
  }

  /* ── Buttons ── */
  .btn-primary {
    padding: 6px 14px; background: var(--accent); color: #fff; border: none;
    border-radius: var(--radius); font-size: 13px; font-weight: 600; cursor: pointer;
  }
  .btn-primary:hover { filter: brightness(1.1); }
  .btn-outline {
    padding: 5px 12px; background: none; border: 1px solid var(--border);
    border-radius: var(--radius); color: var(--text-primary); font-size: 13px; cursor: pointer;
  }
  .btn-outline:hover { background: var(--bg-secondary); }
  .btn-sm { padding: 4px 10px; font-size: 12px; }
</style>
