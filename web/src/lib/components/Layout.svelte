<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();
</script>

<div class="page-layout">
  {@render children()}
</div>

<style>
  .page-layout {
    max-width: 1280px;
    margin: 0 auto;
    padding: 24px 32px;
    min-height: calc(100vh - 62px);
  }
</style>
