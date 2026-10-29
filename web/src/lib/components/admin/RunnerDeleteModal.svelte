<script lang="ts">
  // Delete-runner confirmation modal — self-contained: performs the delete
  // via the runners API, reports failure via toast.
  import Modal from '$lib/components/Modal.svelte';
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

<Modal
  open={true}
  title={t('admin.runners.delete_confirm')}
  submitLabel={deleting ? t('common.loading') : t('common.delete')}
  submitVariant="danger"
  busy={deleting}
  {onClose}
  onSubmit={handleDelete}
>
  <p class="message">{t('admin.runners.delete_warning', { name: runner.name })}</p>
</Modal>

<style>
  .message {
    margin: 0;
    color: var(--text-secondary);
  }








</style>
