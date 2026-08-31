<script lang="ts">
  import { onMount } from 'svelte';
  import type { Snippet } from 'svelte';

  interface Props {
    /** Whether the modal is currently visible. */
    open: boolean;
    /** Close callback — fired on backdrop click, ESC, or X button. */
    onClose: () => void;
    /** Optional title shown in the header.  Omit to hide the header entirely. */
    title?: string;
    /** Snippet for the primary action button (e.g. save).  Places it in a footer row. */
    submitLabel?: string;
    /** Confirm button style — default `primary`. */
    submitVariant?: 'primary' | 'danger';
    /** Confirm button disabled state. */
    submitDisabled?: boolean;
    /** Submit callback.  If provided, renders a submit button. */
    onSubmit?: () => void;
    /** Close on ESC key? Default true. */
    closeOnEsc?: boolean;
    /** Close on backdrop click? Default true. */
    closeOnBackdrop?: boolean;
    /** Width class (e.g. `modal-wide` for a wider dialog). */
    class?: string;
    /** Modal body content (Svelte snippet). */
    children?: Snippet;
  }

  let {
    open,
    onClose,
    title,
    submitLabel = 'OK',
    submitVariant = 'primary',
    submitDisabled = false,
    onSubmit,
    closeOnEsc = true,
    closeOnBackdrop = true,
    class: className = '',
    children,
  }: Props = $props();

  function handleKeydown(e: KeyboardEvent) {
    if (closeOnEsc && e.key === 'Escape') {
      e.stopPropagation();
      onClose();
    }
  }

  function handleBackdrop(e: MouseEvent) {
    if (closeOnBackdrop && e.target === e.currentTarget) {
      onClose();
    }
  }

  // Lock body scroll when open (only on client).
  $effect(() => {
    if (open && typeof document !== 'undefined') {
      const prev = document.body.style.overflow;
      document.body.style.overflow = 'hidden';
      return () => {
        document.body.style.overflow = prev;
      };
    }
  });

  onMount(() => {
    window.addEventListener('keydown', handleKeydown);
    return () => window.removeEventListener('keydown', handleKeydown);
  });
</script>

{#if open}
  <div class="modal-backdrop" onclick={handleBackdrop} role="dialog" aria-modal="true" aria-labelledby={title ? 'modal-title' : undefined}>
    <div class="modal-content {className}">
      {#if title || onClose}
        <header class="modal-header">
          {#if title}
            <h2 id="modal-title" class="modal-title">{title}</h2>
          {/if}
          <button class="modal-close" onclick={onClose} aria-label="Close">×</button>
        </header>
      {/if}

      <div class="modal-body">
        {#if children}{@render children()}{/if}
      </div>

      {#if onSubmit}
        <footer class="modal-footer">
          <button class="btn-secondary" onclick={onClose} type="button">Cancel</button>
          <button
            class="btn-{submitVariant}"
            onclick={onSubmit}
            disabled={submitDisabled}
            type="button"
          >
            {submitLabel}
          </button>
        </footer>
      {/if}
    </div>
  </div>
{/if}

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 10000;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: rgba(0, 0, 0, 0.6);
    animation: fade-in 0.15s ease-out;
  }

  .modal-content {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.4);
    width: min(480px, 100%);
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    animation: modal-in 0.2s ease-out;
  }

  .modal-content.modal-wide { width: min(680px, 100%); }
  .modal-content.modal-narrow { width: min(360px, 100%); }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid var(--border);
  }

  .modal-title {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .modal-close {
    border: 0;
    background: none;
    color: var(--text-muted);
    font-size: 22px;
    line-height: 1;
    cursor: pointer;
    padding: 0 4px;
  }

  .modal-close:hover { color: var(--text-primary); }

  .modal-body {
    flex: 1;
    overflow-y: auto;
    padding: 20px;
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    padding: 14px 20px;
    border-top: 1px solid var(--border);
  }

  @keyframes fade-in {
    from { opacity: 0; }
    to   { opacity: 1; }
  }

  @keyframes modal-in {
    from { opacity: 0; transform: translateY(-8px) scale(0.98); }
    to   { opacity: 1; transform: translateY(0) scale(1); }
  }

  .btn-primary {
    padding: 8px 16px;
    background: var(--accent);
    color: #fff;
    border: 0;
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
  }

  .btn-primary:hover:not(:disabled) { background: var(--accent-hover); }
  .btn-primary:disabled { opacity: 0.6; cursor: not-allowed; }

  .btn-danger {
    padding: 8px 16px;
    background: var(--red);
    color: #fff;
    border: 0;
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
  }

  .btn-danger:hover:not(:disabled) { background: var(--red-dim); }
  .btn-danger:disabled { opacity: 0.6; cursor: not-allowed; }

  .btn-secondary {
    padding: 8px 16px;
    background: transparent;
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 14px;
    cursor: pointer;
  }

  .btn-secondary:hover { background: var(--bg-hover); }
</style>
