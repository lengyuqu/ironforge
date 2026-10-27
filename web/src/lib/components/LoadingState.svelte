<script lang="ts">
  import LoadingSpinner from './LoadingSpinner.svelte';
  import { createT } from '$lib/i18n';

  interface Props {
    /** Override the default "Loading..." text. */
    label?: string;
    /** Render on one line without the vertical padding (for use inside cards/tables). */
    compact?: boolean;
  }

  let { label, compact = false }: Props = $props();

  const t = createT();
</script>

<div class="loading-state" class:compact role="status" aria-live="polite">
  <LoadingSpinner size={compact ? 16 : 22} decorative />
  <span>{label ?? t('common.loading')}</span>
</div>

<style>
  .loading-state {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 40px 16px;
    color: var(--text-secondary);
    font-size: 14px;
  }

  .loading-state.compact {
    justify-content: flex-start;
    padding: 8px 0;
    font-size: 13px;
  }
</style>
