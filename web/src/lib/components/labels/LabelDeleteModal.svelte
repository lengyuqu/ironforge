<script lang="ts">
  // Delete-label confirmation modal — self-contained: performs the delete via
  // the labels API, reports success/failure via toast.
  import { labels } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import type { Label } from '$lib/types/entities';

  interface Props {
    owner: string;
    repo: string;
    label: Label;
    onClose: () => void;
    onDeleted: () => void;
  }

  let { owner, repo, label, onClose, onDeleted }: Props = $props();

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
      await labels.delete(owner, repo, label.id);
      toast.success(t('settings.delete_label', 'Label deleted'));
      onClose();
      onDeleted();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e, t('errors.delete_failed', 'Delete failed')));
    } finally {
      deleting = false;
    }
  }
</script>

<div
  class="form-overlay"
  onclick={onClose}
  role="button"
  tabindex="0"
  onkeydown={closeByKey}
>
  <div class="form-modal" role="dialog" aria-modal="true" tabindex="-1">
    <h2>Confirm Delete</h2>
    <p>{t('settings.confirm_delete_label')}</p>
    <p><strong>{label.name}</strong></p>

    <div class="form-actions">
      <button class="btn btn-outline" onclick={onClose} disabled={deleting}>
        Cancel
      </button>
      <button class="btn btn-danger" onclick={handleDelete} disabled={deleting}>
        {deleting ? 'Deleting...' : 'Delete'}
      </button>
    </div>
  </div>
</div>

<style>
  .form-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .form-modal {
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 2rem;
    max-width: 500px;
    width: 90%;
    max-height: 90vh;
    overflow-y: auto;
  }

  .form-modal h2 {
    margin: 0 0 1.5rem 0;
    color: var(--text-primary);
    font-size: 1.25rem;
  }

  .form-modal p {
    color: var(--text-secondary);
    margin-bottom: 1rem;
  }

  .form-actions {
    display: flex;
    gap: 0.75rem;
    justify-content: flex-end;
    margin-top: 1.5rem;
  }
</style>
