<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { boards } from '$lib/api/client.svelte';
  import type { Board, BoardFull } from '$lib/types/entities';
  import BoardSwitcher from '$lib/components/boards/BoardSwitcher.svelte';
  import BoardCreateForm from '$lib/components/boards/BoardCreateForm.svelte';
  import ColumnCreateForm from '$lib/components/boards/ColumnCreateForm.svelte';
  import BoardColumn from '$lib/components/boards/BoardColumn.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  let boardList = $state<Board[]>([]);
  let activeBoardId = $state<number | null>(null);
  let activeBoard = $state<BoardFull | null>(null);
  let loading = $state(true);
  let error = $state('');

  let showCreateBoard = $state(false);
  let showAddColumn = $state(false);

  $effect(() => {
    loadBoards();
  });

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
      error = e.message || t('errors.load_failed');
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

  async function handleDeleteBoard(id: number) {
    if (!confirm(t('board.confirmDelete'))) return;
    try {
      await boards.delete(owner, repo, id);
      activeBoard = null;
      activeBoardId = null;
      await loadBoards();
    } catch (e: any) {
      error = e.message || t('errors.load_failed');
    }
  }
</script>

<svelte:head>
  <title>Board · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="board" />

  <div class="page-header">
    <h1>{t('board.title')}</h1>
    <button class="btn-primary" onclick={() => (showCreateBoard = true)}>
      + {t('board.createBoard')}
    </button>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="loading-text">{t('common.loading')}…</p>
  {:else if boardList.length === 0}
    <p class="empty-text">{t('board.noBoards')}</p>
  {:else}
    <div class="board-toolbar">
      <BoardSwitcher
        boards={boardList}
        activeBoardId={activeBoardId}
        onSelect={handleSelectBoard}
        onAddBoard={() => (showCreateBoard = !showCreateBoard)}
      />
      <div class="toolbar-actions">
        <button class="btn-outline btn-sm" onclick={() => (showAddColumn = !showAddColumn)}>
          + {t('board.addColumn')}
        </button>
        {#if activeBoardId != null}
          <button
            class="btn-outline btn-sm btn-danger"
            onclick={() => handleDeleteBoard(activeBoardId!)}
          >
            {t('board.deleteBoard')}
          </button>
        {/if}
      </div>
    </div>

    {#if showCreateBoard}
      <BoardCreateForm {owner} {repo} onCreated={handleBoardCreated} onCancel={() => (showCreateBoard = false)} />
    {/if}

    {#if showAddColumn && activeBoardId != null}
      <ColumnCreateForm
        {owner}
        {repo}
        boardId={activeBoardId}
        onCreated={handleColumnCreated}
        onCancel={() => (showAddColumn = false)}
      />
    {/if}

    {#if activeBoard?.columns}
      <div class="board-container">
        {#each activeBoard.columns as { column, cards } (column.id)}
          <BoardColumn
            {owner}
            {repo}
            boardId={activeBoard!.board.id}
            {column}
            {cards}
            onRefresh={async () => {
              if (activeBoardId != null) await loadBoard(activeBoardId);
            }}
          />
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .page-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 20px;
  }

  h1 {
    font-size: 24px;
    font-weight: 600;
    margin: 0;
  }

  .loading-text,
  .empty-text {
    color: var(--text-secondary, #666);
    text-align: center;
    padding: 48px;
  }

  .board-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
    flex-wrap: wrap;
    gap: 8px;
  }

  .toolbar-actions {
    display: flex;
    gap: 8px;
  }

  .board-container {
    display: flex;
    gap: 16px;
    overflow-x: auto;
    padding-bottom: 16px;
    align-items: flex-start;
  }

  .error-banner {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 10px 12px;
    border-radius: 6px;
    margin-bottom: 16px;
  }

  .btn-primary {
    padding: 6px 14px;
    background: var(--accent, #2563eb);
    color: #fff;
    border: none;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  .btn-primary:hover {
    filter: brightness(1.1);
  }

  .btn-outline {
    padding: 5px 12px;
    background: none;
    border: 1px solid var(--border, #d1d5db);
    border-radius: 6px;
    color: var(--text-primary, #333);
    font-size: 13px;
    cursor: pointer;
  }

  .btn-outline:hover {
    background: var(--bg-secondary, #f3f4f6);
  }

  .btn-danger {
    color: #dc2626;
    border-color: rgba(220, 38, 38, 0.4);
  }

  .btn-sm {
    padding: 4px 10px;
    font-size: 12px;
  }
</style>
