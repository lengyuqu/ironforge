<script lang="ts">
  import { toast, type ToastItem } from './toast.svelte';

  function dismiss(item: ToastItem) {
    toast.dismiss(item.id);
  }
</script>

{#each toast.items as item, i (item.id)}
  <div
    class="toast toast-{item.type}"
    role="status"
    aria-live="polite"
    style="top: calc(80px + {i} * 64px);"
  >
    <span class="toast-icon" aria-hidden="true">
      {#if item.type === 'success'}✓{:else if item.type === 'error'}✕{:else if item.type === 'warning'}!{:else}ⓘ{/if}
    </span>
    <span class="toast-message">{item.message}</span>
    <button class="toast-close" onclick={() => dismiss(item)} aria-label="Dismiss">×</button>
  </div>
{/each}

<style>
  .toast {
    position: fixed;
    right: 20px;
    z-index: 9999;
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 240px;
    max-width: 420px;
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-secondary);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3);
    font-size: 14px;
    color: var(--text-primary);
    animation: toast-in 0.2s ease-out;
  }

  .toast-icon {
    flex: none;
    font-weight: 700;
    font-size: 14px;
  }

  .toast-message {
    flex: 1;
    line-height: 1.4;
    word-break: break-word;
  }

  .toast-close {
    flex: none;
    border: 0;
    background: none;
    color: var(--text-muted);
    font-size: 18px;
    line-height: 1;
    cursor: pointer;
    padding: 0 4px;
  }

  .toast-close:hover {
    color: var(--text-primary);
  }

  .toast-success { border-color: var(--green); }
  .toast-success .toast-icon { color: var(--green); }

  .toast-error { border-color: var(--red); }
  .toast-error .toast-icon { color: var(--red); }

  .toast-warning { border-color: var(--yellow); }
  .toast-warning .toast-icon { color: var(--yellow); }

  .toast-info .toast-icon { color: var(--accent); }

  @keyframes toast-in {
    from { opacity: 0; transform: translateX(20px); }
    to   { opacity: 1; transform: translateX(0); }
  }
</style>
