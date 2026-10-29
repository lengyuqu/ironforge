<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    /** Confirmation text to show. */
    message?: string;
    /** Rich content, used instead of `message` (e.g. text with a link). */
    children?: Snippet;
  }

  let { message, children }: Props = $props();
</script>

<div class="success-banner" role="status">
  {#if children}{@render children()}{:else}{message}{/if}
</div>

<style>
  .success-banner {
    margin-bottom: 16px;
    padding: 10px 14px;
    border: 1px solid var(--green-dim);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--green-dim) 14%, transparent);
    color: var(--green);
    font-size: 13px;
    line-height: 1.5;
    word-break: break-word;
  }
</style>
