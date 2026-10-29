<script lang="ts">
  // Delete-label confirmation modal — self-contained: performs the delete via
  // the labels API, reports success/failure via toast.
  import Modal from '$lib/components/Modal.svelte';
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

<Modal
  open={true}
  title={t('settings.confirm_delete_label_title', 'Confirm Delete')}
  submitLabel={deleting ? t('common.loading') : t('common.delete')}
  submitVariant="danger"
  busy={deleting}
  {onClose}
  onSubmit={handleDelete}
>
  <p class="message">{t('settings.confirm_delete_label')}</p>
  <p class="target"><strong>{label.name}</strong></p>
</Modal>

<style>




  .message,
  .target {
    color: var(--text-secondary);
    margin: 0 0 0.75rem;
  }
</style>
