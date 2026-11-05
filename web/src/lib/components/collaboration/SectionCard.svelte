<script lang="ts">
  import type { Snippet } from 'svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingSpinner from '$lib/components/LoadingSpinner.svelte';

  interface Props {
    title: string;
    /** One line stating what kind of data this is (declaration, hint, derived...). */
    note?: string;
    loading?: boolean;
    error?: string;
    count?: number | null;
    children: Snippet;
    actions?: Snippet;
  }

  let { title, note, loading = false, error = '', count = null, children, actions }: Props = $props();
</script>

<section class="card surface">
  <header>
    <h2>{title}{#if count !== null}<span class="count">{count}</span>{/if}</h2>
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
  </header>
  {#if note}<p class="note">{note}</p>{/if}
  {#if error}<ErrorBanner message={error} />{/if}
  {#if loading}
    <LoadingSpinner />
  {:else}
    {@render children()}
  {/if}
</section>

<style>
  .card { padding: 16px 20px; margin-bottom: 20px; min-width: 0; }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  h2 { margin: 0; font-size: 16px; }
  .count {
    margin-left: 8px; padding: 0 8px; border-radius: 999px; font-size: 12px;
    background: var(--bg-tertiary); color: var(--text-secondary); font-weight: 600;
  }
  .note { margin: 4px 0 12px; font-size: 12px; color: var(--text-muted); }
  .actions { display: flex; gap: 8px; align-items: center; }
</style>
