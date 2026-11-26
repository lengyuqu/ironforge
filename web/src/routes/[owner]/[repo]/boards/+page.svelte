<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import { confirmDialog } from '$lib/components/confirm.svelte';
  import { page } from '$app/state';
  import { boards } from '$lib/api/client.svelte';
  import type { Board, BoardFull } from '$lib/types/entities';
  import BoardSwitcher from '$lib/components/boards/BoardSwitcher.svelte';
  import BoardCreateForm from '$lib/components/boards/BoardCreateForm.svelte';
  import ColumnCreateForm from '$lib/components/boards/ColumnCreateForm.svelte';
  import BoardColumn from '$lib/components/boards/BoardColumn.svelte';
  import { createT } from '$lib/i18n';
  import { createLatest } from '$lib/utils/latest';

  const t = createT();
  const boardsLatest = createLatest();
  const boardLatest = createLatest();

  let owner = $derived(page.params.owner!);
  let repo = $derived(page.params.repo!);

  let boardList = $state<Board[]>([]);
  let activeBoardId = $state<number | null>(null);
  let activeBoard = $state<BoardFull | null>(null);
  let loading = $state(true);
  let error = $state('');

  let showCreateBoard = $state(false);
  let showAddColumn = $state(false);

  $effect(() => {
    void loadBoards();
  });

  async function loadBoards() {
    const reqOwner = owner;
    const reqRepo = repo;
    boardLatest.invalidate();
    const isCurrent = boardsLatest.begin();
    loading = true;
    error = '';
    try {
      const list = await boards.list(reqOwner, reqRepo);
      if (!isCurrent()) return;
      boardList = list;
      if (boardList.length > 0) {
        activeBoardId = boardList.some((b) => b.id === activeBoardId) ? activeBoardId : boardList[0].id;
        await loadBoard(activeBoardId!);
        if (!isCurrent()) return;
      } else {
        activeBoardId = null;
        activeBoard = null;
      }
    } catch (e) {
      if (!isCurrent()) return;
      error = toErrorMessage(e, t('errors.load_failed'));
    } finally {
      if (isCurrent()) {
        loading = false;
      }
    }
  }

  async function loadBoard(id: number) {
    const reqOwner = owner;
    const reqRepo = repo;
    const isCurrent = boardLatest.begin();
    const board = await boards.get(reqOwner, reqRepo, id);
    if (!isCurrent()) return;
    activeBoard = board;
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
    if (!(await confirmDialog({ variant: 'danger', message: t('board.confirmDelete') }))) return;
    try {
      await boards.delete(owner, repo, id);
      activeBoard = null;
      activeBoardId = null;
      await loadBoards();
    } catch (e) {
      error = toErrorMessage(e, t('errors.load_failed'));
    }
  }
</script>

<svelte:head>
  <title>{t('repo.tabs.board')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-header">
  <h1>{t('board.title')}</h1>
  <button class="btn-primary" onclick={() => (showCreateBoard = true)}>
    + {t('board.createBoard')}
  </button>
</div>

{#if error}
  <ErrorBanner message={error} />
{/if}

{#if loading}
  <LoadingState />
{:else if boardList.length === 0}
  <EmptyState icon="📋" title={t('board.noBoards')} description={t('board.noBoardsHint')} />
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

<style>

  h1 {
    margin: 0;
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
</style>
