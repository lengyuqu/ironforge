<script lang="ts">
  import { getBanner, clearBanner } from '$lib/stores/instance.svelte';

  let { message, type } = $derived(getBanner());
</script>

{#if message}
  <div class="banner" class:info={type === 'info'} class:warning={type === 'warning'} class:error={type === 'error'}>
    <span class="banner-icon">
      {#if type === 'warning'}⚠️{:else if type === 'error'}🚫{:else}ℹ️{/if}
    </span>
    <span class="banner-text">{message}</span>
    <button class="banner-close" onclick={clearBanner}>✕</button>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    font-size: 13px;
    font-weight: 500;
    z-index: 50;
  }
  .info { background: #dbeafe; color: #1e40af; border-bottom: 1px solid #bfdbfe; }
  .warning { background: #fef3c7; color: #92400e; border-bottom: 1px solid #fde68a; }
  .error { background: #fce4ec; color: #c62828; border-bottom: 1px solid #f8bbd0; }
  .banner-text { flex: 1; }
  .banner-close {
    background: none; border: none; cursor: pointer;
    font-size: 14px; opacity: 0.6; padding: 2px 6px; border-radius: 3px;
  }
  .banner-close:hover { opacity: 1; background: rgba(0,0,0,0.05); }
</style>
