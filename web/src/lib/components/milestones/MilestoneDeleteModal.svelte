<script lang="ts">
  // Delete-milestone confirmation modal — self-contained: performs the
  // delete via the milestones API, reports success/failure via toast.
  // Note: backend cascade behaviour for linked issues is pending
  // confirmation (gap-analysis §A5) — the copy assumes the association
  // is removed.
  import Modal from '$lib/components/Modal.svelte';
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

<Modal
  open={true}
  title={t('settings.confirm_delete_milestone_title', 'Confirm Delete')}
  submitLabel={deleting ? t('common.loading') : t('settings.delete_milestone')}
  submitVariant="danger"
  busy={deleting}
  {onClose}
  onSubmit={handleDelete}
>
  <p class="message">{t('settings.confirm_delete_milestone')}</p>
  <p class="target"><strong>{milestone.title}</strong></p>
</Modal>

<style>






  .message,
  .target {
    color: var(--text-secondary);
    margin: 0 0 0.75rem;
  }
</style>
