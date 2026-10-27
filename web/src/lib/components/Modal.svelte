<script lang="ts" module>
  // Stack of open modals so that only the topmost one reacts to Escape / Tab.
  const openStack: symbol[] = [];
  let modalSeq = 0;
</script>

<script lang="ts">
  import { tick } from 'svelte';
  import type { Snippet } from 'svelte';
  import { createT } from '$lib/i18n';

  interface Props {
    /** Whether the modal is currently visible. */
    open: boolean;
    /** Close callback — fired on backdrop click, ESC, or X button. */
    onClose: () => void;
    /** Optional title shown in the header.  Omit to hide the header entirely. */
    title?: string;
    /** Label of the primary action button (e.g. save).  Defaults to "Confirm". */
    submitLabel?: string;
    /** Confirm button style — default `primary`. */
    submitVariant?: 'primary' | 'danger';
    /** Confirm button disabled state. */
    submitDisabled?: boolean;
    /** Submit callback.  If provided, renders a footer with cancel + submit buttons. */
    onSubmit?: () => void;
    /** Close on ESC key? Default true. */
    closeOnEsc?: boolean;
    /** Close on backdrop click? Default true. */
    closeOnBackdrop?: boolean;
    /** A request is in flight: block every dismissal path and disable Cancel. */
    busy?: boolean;
    /** Width class (e.g. `modal-wide` for a wider dialog). */
    class?: string;
    /** Modal body content (Svelte snippet). */
    children?: Snippet;
  }

  let {
    open,
    onClose,
    title,
    submitLabel,
    submitVariant = 'primary',
    submitDisabled = false,
    onSubmit,
    closeOnEsc = true,
    closeOnBackdrop = true,
    busy = false,
    class: className = '',
    children,
  }: Props = $props();

  const t = createT();
  const token = Symbol('modal');
  const titleId = `modal-title-${++modalSeq}`;
  const FOCUSABLE =
    'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

  let contentEl = $state<HTMLDivElement | null>(null);
  let pressedInside = false;

  function requestClose() {
    if (!busy) onClose();
  }

  function isTop() {
    return openStack[openStack.length - 1] === token;
  }

  function focusables(): HTMLElement[] {
    return contentEl ? Array.from(contentEl.querySelectorAll<HTMLElement>(FOCUSABLE)) : [];
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!isTop()) return;
    if (e.key === 'Escape' && closeOnEsc) {
      e.stopPropagation();
      requestClose();
      return;
    }
    if (e.key === 'Tab') {
      // Keep keyboard focus inside the dialog.
      const items = focusables();
      if (items.length === 0) {
        e.preventDefault();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement;
      if (e.shiftKey && (active === first || !contentEl?.contains(active))) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && (active === last || !contentEl?.contains(active))) {
        e.preventDefault();
        first.focus();
      }
    }
  }

  function handleContentKeydown(e: KeyboardEvent) {
    // Enter inside a single-line field submits, like a native form would.
    if (e.key !== 'Enter' || !onSubmit || submitDisabled || e.isComposing) return;
    const el = e.target as HTMLElement;
    if (el.tagName === 'INPUT' && (el as HTMLInputElement).type !== 'checkbox') {
      e.preventDefault();
      onSubmit();
    }
  }

  function handleMouseDown(e: MouseEvent) {
    pressedInside = e.target !== e.currentTarget;
  }

  function handleBackdrop(e: MouseEvent) {
    // A drag that starts in the dialog (e.g. selecting text) and ends on the
    // backdrop must not dismiss it.
    if (closeOnBackdrop && !pressedInside && e.target === e.currentTarget) {
      requestClose();
    }
    pressedInside = false;
  }

  // While open: lock body scroll, own the Escape/Tab keys, move focus into the
  // dialog and put it back on the trigger when the dialog closes.
  $effect(() => {
    if (!open || typeof document === 'undefined') return;

    const prevOverflow = document.body.style.overflow;
    const opener = document.activeElement as HTMLElement | null;
    document.body.style.overflow = 'hidden';
    openStack.push(token);
    window.addEventListener('keydown', handleKeydown);

    tick().then(() => {
      if (!contentEl || contentEl.contains(document.activeElement)) return;
      const preferred =
        contentEl.querySelector<HTMLElement>('[data-autofocus]') ??
        contentEl.querySelector<HTMLElement>(
          '.modal-body input:not([type="hidden"]), .modal-body textarea, .modal-body select',
        ) ??
        contentEl.querySelector<HTMLElement>(
          // Destructive dialogs start on Cancel so a stray Enter is harmless.
          submitVariant === 'danger' ? '.modal-footer button:first-child' : '.modal-footer button:last-child',
        ) ??
        focusables()[0];
      preferred?.focus();
    });

    return () => {
      window.removeEventListener('keydown', handleKeydown);
      const i = openStack.indexOf(token);
      if (i >= 0) openStack.splice(i, 1);
      document.body.style.overflow = prevOverflow;
      if (opener && document.contains(opener)) opener.focus();
    };
  });
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="modal-backdrop"
    onmousedown={handleMouseDown}
    onclick={handleBackdrop}
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-labelledby={title ? titleId : undefined}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="modal-content {className}" bind:this={contentEl} onkeydown={handleContentKeydown}>
      <header class="modal-header">
        {#if title}
          <h2 id={titleId} class="modal-title">{title}</h2>
        {/if}
        <button class="modal-close" onclick={requestClose} disabled={busy} aria-label={t('common.close')} type="button">×</button>
      </header>

      <div class="modal-body">
        {#if children}{@render children()}{/if}
      </div>

      {#if onSubmit}
        <footer class="modal-footer">
          <button class="btn-secondary" onclick={requestClose} disabled={busy} type="button">{t('common.cancel')}</button>
          <button
            class="btn-{submitVariant}"
            onclick={onSubmit}
            disabled={submitDisabled}
            type="button"
          >
            {submitLabel ?? t('common.confirm')}
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
    max-height: min(80vh, 100%);
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

  .modal-close:hover:not(:disabled) { color: var(--text-primary); }
  .modal-close:disabled,
  .btn-secondary:disabled { opacity: 0.5; cursor: not-allowed; }

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

  .btn-primary:focus-visible,
  .btn-danger:focus-visible,
  .btn-secondary:focus-visible,
  .modal-close:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
