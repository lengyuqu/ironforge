<script lang="ts">
  // Delete-milestone confirmation modal — self-contained: performs the
  // delete via the milestones API, reports success/failure via toast.
  // Note: backend cascade behaviour for linked issues is pending
  // confirmation (gap-analysis §A5) — the copy assumes the association
  // is removed.
  import { milestones } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import type { Milestone } from '$lib/types/entities';

  interface Props {
    owner: string;
    repo: string;
    milestone: Milestone;
    onClose: () => void;
    onDeleted: () => void;
  }

  let { owner, repo, milestone, onClose, onDeleted }: Props = $props();

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
      await milestones.delete(owner, repo, milestone.id);
      toast.success(t('settings.delete_milestone', 'Milestone deleted'));
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
  <div
    class="form-modal"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
  >
    <h2>{t('settings.confirm_delete_milestone_title', 'Confirm Delete')}</h2>
    <p>{t('settings.confirm_delete_milestone')}</p>
    <p><strong>{milestone.title}</strong></p>

    <div class="form-actions">
      <button class="btn btn-outline" onclick={onClose} disabled={deleting}>
        {t('common.cancel', 'Cancel')}
      </button>
      <button class="btn btn-danger" onclick={handleDelete} disabled={deleting}>
        {deleting ? t('common.loading') : t('settings.delete_milestone')}
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
    max-width: 420px;
    width: 90%;
  }

  .form-modal h2 {
    margin: 0 0 1rem 0;
    color: var(--text-primary);
    font-size: 1.25rem;
  }

  .form-modal p {
    color: var(--text-primary);
    font-size: 0.95rem;
    margin: 0 0 0.5rem 0;
  }

  .form-actions {
    display: flex;
    gap: 0.75rem;
    justify-content: flex-end;
    margin-top: 1.5rem;
  }

  .btn-danger {
    background: #d73a49;
    color: #fff;
    border: 1px solid #d73a49;
    border-radius: var(--radius);
    padding: 0.4rem 1rem;
    font-size: 0.9rem;
    cursor: pointer;
  }

  .btn-danger:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
