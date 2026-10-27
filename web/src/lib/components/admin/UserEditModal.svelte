<script lang="ts">
  import Modal from '$lib/components/Modal.svelte';
  import { admin, type AdminUser } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    user,
    onClose,
    onSaved,
  }: {
    user: AdminUser;
    onClose: () => void;
    onSaved: () => void | Promise<void>;
  } = $props();

  let editDisplayName = $state(user.display_name || '');
  let editBio = $state(user.bio || '');
  let editIsAdmin = $state(user.is_admin);
  let editIsActive = $state(user.is_active);
  let saving = $state(false);
  let error = $state('');

  async function handleSave() {
    saving = true;
    error = '';
    try {
      await admin.updateUser(user.id, {
        display_name: editDisplayName || undefined,
        bio: editBio || undefined,
        is_admin: editIsAdmin,
        is_active: editIsActive,
      });
      onClose();
      await onSaved();
    } catch (e: any) {
      error = toErrorMessage(e, t('errors.load_failed'));
    } finally {
      saving = false;
    }
  }
</script>

<Modal
  open={true}
  title={t('admin.users.edit', { username: user.username })}
  submitLabel={saving ? t('common.loading') : t('common.save')}
  busy={saving}
  {onClose}
  onSubmit={handleSave}
>
  {#if error}
    <div class="error" role="alert">{error}</div>
  {/if}

  <div class="form-group">
    <label for="admin-user-display-name">{t('admin.users.display_name')}</label>
    <input id="admin-user-display-name" type="text" bind:value={editDisplayName} />
  </div>

  <div class="form-group">
    <label for="admin-user-bio">{t('admin.users.bio')}</label>
    <textarea id="admin-user-bio" bind:value={editBio} rows="3"></textarea>
  </div>

  <div class="form-group">
    <label class="checkbox-label">
      <input type="checkbox" bind:checked={editIsAdmin} />
      {t('admin.users.is_admin')}
    </label>
    <label class="checkbox-label">
      <input type="checkbox" bind:checked={editIsActive} />
      {t('admin.users.is_active')}
    </label>
  </div>
</Modal>

<style>

  .form-group {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    margin-bottom: 1rem;
  }

  label {
    font-size: 0.9rem;
    color: var(--text-secondary);
    font-weight: 500;
  }

  input[type='text'],
  textarea {
    padding: 0.5rem 0.7rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-primary);
    color: var(--text-primary);
    font-size: 0.95rem;
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    cursor: pointer;
    color: var(--text-primary);
  }

  .error {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 0.5rem 0.75rem;
    border-radius: 6px;
    margin-bottom: 1rem;
  }
</style>
