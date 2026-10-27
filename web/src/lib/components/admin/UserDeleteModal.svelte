<script lang="ts">
  import Modal from '$lib/components/Modal.svelte';
  import { admin, type AdminUser } from '$lib/api/client.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    user,
    onClose,
    onDeleted,
  }: {
    user: AdminUser;
    onClose: () => void;
    onDeleted: () => void | Promise<void>;
  } = $props();

  let saving = $state(false);
  let error = $state('');

  async function handleDelete() {
    saving = true;
    error = '';
    try {
      await admin.deleteUser(user.id);
      onClose();
      await onDeleted();
    } catch (e: any) {
      error = toErrorMessage(e, t('errors.load_failed'));
    } finally {
      saving = false;
    }
  }
</script>

<Modal
  open={true}
  title={t('admin.users.delete_confirm')}
  submitLabel={saving ? t('common.loading') : t('common.delete')}
  submitVariant="danger"
  busy={saving}
  {onClose}
  onSubmit={handleDelete}
>
  <p class="message">{t('admin.users.delete_warning', { username: user.username })}</p>
  {#if error}
    <div class="error" role="alert">{error}</div>
  {/if}
</Modal>

<style>
  .message {
    margin: 0 0 12px;
    color: var(--text-secondary);
  }

  .error {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 0.5rem 0.75rem;
    border-radius: 6px;
    margin-bottom: 1rem;
  }
</style>
