<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    /** Error text to show. */
    message: string;
    /** Optional action, e.g. a retry button. */
    children?: Snippet;
  }

  let { message, children }: Props = $props();
</script>

<div class="error-banner" role="alert">
  <span class="error-message">{message}</span>
  {#if children}<span class="error-actions">{@render children()}</span>{/if}
</div>

<style>
  .error-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 16px;
    padding: 10px 14px;
    border: 1px solid var(--red-dim);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--red) 12%, transparent);
    color: var(--red);
    font-size: 13px;
    line-height: 1.5;
  }

  .error-message {
    min-width: 0;
    word-break: break-word;
  }

  .error-actions {
    flex: none;
  }
</style>
