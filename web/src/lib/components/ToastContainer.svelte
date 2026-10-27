<script lang="ts">
  import { toast, type ToastItem } from './toast.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  function dismiss(item: ToastItem) {
    toast.dismiss(item.id);
  }
</script>

<div class="toast-stack surface" aria-live="polite">
  {#each toast.items as item (item.id)}
    <div
      class="toast toast-{item.type}"
      role={item.type === 'error' ? 'alert' : 'status'}
      onmouseenter={() => toast.pause(item.id)}
      onmouseleave={() => toast.resume(item.id)}
      onfocusin={() => toast.pause(item.id)}
      onfocusout={() => toast.resume(item.id)}
    >
      <span class="toast-icon surface" aria-hidden="true">
        {#if item.type === 'success'}✓{:else if item.type === 'error'}✕{:else if item.type === 'warning'}!{:else}ⓘ{/if}
      </span>
      <span class="toast-message surface">{item.message}</span>
      <button class="toast-close surface" onclick={() => dismiss(item)} aria-label={t('common.dismiss')}>×</button>
    </div>
  {/each}
</div>

<style>
  .toast-stack {
    position: fixed;
    top: 72px;
    right: 20px;
    z-index: 11000;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: min(420px, calc(100vw - 40px));
    pointer-events: none;
  }

  .toast {
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    min-width: 240px;
    padding: 12px 14px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3);
    font-size: 14px;
    color: var(--text-primary);
    animation: toast-in 0.2s ease-out;
  }

  @media (max-width: 600px) {
    .toast-stack {
      left: 12px;
      right: 12px;
      max-width: none;
    }
    .toast { min-width: 0; }
  }

  .toast-icon {
    flex: none;
    font-weight: 700;
    font-size: 14px;
  }

  .toast-message {
    flex: 1;
    line-height: 1.4;
    padding-top: 1px;
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
