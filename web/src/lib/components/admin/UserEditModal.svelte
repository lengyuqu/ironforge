<script lang="ts">
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

  function closeByKey(e: KeyboardEvent) {
    if (e.key === 'Escape' || e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      onClose();
    }
  }

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

<div class="modal-overlay" onclick={onClose} role="button" tabindex="0" onkeydown={closeByKey}>
  <div
    class="modal"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
  >
    <h2>{t('admin.users.edit', { username: user.username })}</h2>

    {#if error}
      <div class="error">{error}</div>
    {/if}

    <div class="form-group">
      <label for="admin-user-display-name">Display Name</label>
      <input id="admin-user-display-name" type="text" bind:value={editDisplayName} />
    </div>

    <div class="form-group">
      <label for="admin-user-bio">Bio</label>
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

    <div class="modal-actions">
      <button class="btn-primary" onclick={handleSave} disabled={saving}>
        {saving ? t('common.loading') : t('common.save')}
      </button>
      <button class="btn-secondary" onclick={onClose}>{t('common.cancel')}</button>
    </div>
  </div>
</div>

<style>
  h2 {
    margin: 0 0 1rem;
  }

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

  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .modal {
    background: var(--bg-primary);
    padding: 1.5rem;
    border-radius: 10px;
    min-width: 360px;
    max-width: 440px;
  }

  .modal-actions {
    display: flex;
    gap: 0.75rem;
    justify-content: flex-end;
  }

  .btn-primary,
  .btn-secondary {
    padding: 0.5rem 1rem;
    border-radius: 6px;
    border: none;
    cursor: pointer;
    font-size: 0.9rem;
  }

  .btn-primary {
    background: var(--accent);
    color: #fff;
  }

  .btn-secondary {
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--border);
  }

  .error {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 0.5rem 0.75rem;
    border-radius: 6px;
    margin-bottom: 1rem;
  }
</style>
