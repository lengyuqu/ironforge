<script lang="ts">
  interface Props {
    /** Total number of items (from the PaginatedResponse meta). */
    total: number;
    /** Current 1-based page number. */
    page: number;
    /** Number of items per page. */
    perPage: number;
    /** Callback when the user picks a new page. */
    onPageChange: (page: number) => void;
    /** Maximum number of page buttons shown in the middle. Default 5. */
    siblingCount?: number;
  }

  let { total, page, perPage, onPageChange, siblingCount = 5 }: Props = $props();

  const totalPages = $derived(Math.max(1, Math.ceil(total / perPage)));

  /**
   * Build the list of page buttons, with ellipses where pages are skipped.
   * e.g. page=5, totalPages=10, sibling=2  →  [1, '…', 3, 4, 5, 6, 7, '…', 10]
   */
  const pageNumbers = $derived.by(() => {
    const result: (number | '…')[] = [];
    const total = totalPages;
    const current = page;
    const siblings = siblingCount;

    // First + last always included, so window is (siblings*2 + first + last) + current?
    // Simpler approach:
    const left = Math.max(1, current - siblings);
    const right = Math.min(total, current + siblings);

    if (left > 1) {
      result.push(1);
      if (left > 2) result.push('…');
    }
    for (let i = left; i <= right; i++) result.push(i);
    if (right < total) {
      if (right < total - 1) result.push('…');
      result.push(total);
    }
    return result;
  });

  function goto(p: number) {
    if (p >= 1 && p <= totalPages && p !== page) {
      onPageChange(p);
    }
  }

  const start = $derived((page - 1) * perPage + 1);
  const end = $derived(Math.min(page * perPage, total));
</script>

{#if totalPages > 1}
  <nav class="pagination" aria-label="Pagination">
    <button
      class="pg-btn"
      onclick={() => goto(page - 1)}
      disabled={page <= 1}
      aria-label="Previous page"
    >‹</button>

    {#each pageNumbers as p (p)}
      {#if p === '…'}
        <span class="pg-ellipsis">…</span>
      {:else}
        <button
          class="pg-btn {p === page ? 'active' : ''}"
          onclick={() => goto(p)}
          aria-current={p === page ? 'page' : undefined}
        >{p}</button>
      {/if}
    {/each}

    <button
      class="pg-btn"
      onclick={() => goto(page + 1)}
      disabled={page >= totalPages}
      aria-label="Next page"
    >›</button>

    <span class="pg-info">
      {total > 0 ? `${start}-${end} of ${total}` : 'No items'}
    </span>
  </nav>
{/if}

<style>
  .pagination {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 12px 0;
    font-size: 14px;
  }

  .pg-btn {
    min-width: 32px;
    height: 32px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: transparent;
    color: var(--text-secondary);
    font-size: 13px;
    cursor: pointer;
  }

  .pg-btn:hover:not(:disabled):not(.active) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .pg-btn.active {
    background: var(--accent);
    color: #fff;
    border-color: var(--accent);
    cursor: default;
  }

  .pg-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .pg-ellipsis {
    padding: 0 4px;
    color: var(--text-muted);
  }

  .pg-info {
    margin-left: auto;
    color: var(--text-muted);
  }
</style>
