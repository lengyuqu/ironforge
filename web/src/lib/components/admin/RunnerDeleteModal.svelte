<script lang="ts">
  // Delete-runner confirmation modal — self-contained: performs the delete
  // via the runners API, reports failure via toast.
  import { runners } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import type { RunnerListItem } from '$lib/api/client.svelte';

  interface Props {
    runner: RunnerListItem;
    onClose: () => void;
    onDeleted: () => void | Promise<void>;
  }

  let { runner, onClose, onDeleted }: Props = $props();

  const t = createT();

  let deleting = $state(false);

  function closeByKey(e: KeyboardEvent) {
    if (e.key === 'Escape' || e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      onClose();
    }
  }

  async function handleDelete() {
    try {
      deleting = true;
      await runners.delete(runner.id);
      onClose();
      await onDeleted();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e, t('errors.delete_failed', 'Delete failed')));
    } finally {
      deleting = false;
    }
  }
</script>

<div
  class="modal-overlay"
  onclick={onClose}
  role="button"
  tabindex="0"
  onkeydown={closeByKey}
>
  <div
    class="modal"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
  >
    <h2>{t('admin.runners.delete_confirm')}</h2>
    <p>{t('admin.runners.delete_warning', { name: runner.name })}</p>
    <div class="modal-actions">
      <button class="btn-danger" onclick={handleDelete} disabled={deleting}>
        {deleting ? t('common.loading') : t('common.delete')}
      </button>
      <button class="btn-secondary" onclick={onClose}>{t('common.cancel')}</button>
    </div>
  </div>
</div>

<style>
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 1.5rem;
    width: 420px;
    max-width: 90vw;
  }

  .modal h2 {
    margin: 0 0 0.75rem;
    font-size: 1.1rem;
  }

  .modal p {
    color: var(--text-secondary);
    margin: 0 0 1rem;
  }

  .modal-actions {
    display: flex;
    gap: 0.75rem;
    justify-content: flex-end;
    margin-top: 1.25rem;
  }

  .btn-secondary {
    background: var(--bg-primary);
    border: 1px solid var(--border);
    color: var(--text-primary);
    border-radius: 6px;
    padding: 0.45rem 0.8rem;
    cursor: pointer;
  }

  .btn-danger {
    background: rgba(248, 81, 73, 0.15);
    border: 1px solid #f85149;
    color: #f85149;
    border-radius: 4px;
    padding: 0.25rem 0.6rem;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .btn-danger:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
