<script lang="ts">
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

  function closeByKey(e: KeyboardEvent) {
    if (e.key === 'Escape' || e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      onClose();
    }
  }

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

<div class="modal-overlay" onclick={onClose} role="button" tabindex="0" onkeydown={closeByKey}>
  <div class="modal" role="dialog" aria-modal="true" tabindex="-1">
    <h2>{t('admin.users.delete_confirm')}</h2>
    <p>
      {t('admin.users.delete_warning', { username: user.username })}
    </p>
    {#if error}
      <div class="error">{error}</div>
    {/if}
    <div class="modal-actions">
      <button class="btn-danger" onclick={handleDelete} disabled={saving}>
        {saving ? t('common.loading') : t('common.delete')}
      </button>
      <button class="btn-secondary" onclick={onClose}>{t('common.cancel')}</button>
    </div>
  </div>
</div>

<style>
  h2 {
    margin: 0 0 0.75rem;
  }

  p {
    color: var(--text-secondary);
    font-size: 0.95rem;
    margin: 0 0 1rem;
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
    min-width: 340px;
    max-width: 420px;
  }

  .modal-actions {
    display: flex;
    gap: 0.75rem;
    justify-content: flex-end;
  }

  .btn-danger,
  .btn-secondary {
    padding: 0.5rem 1rem;
    border-radius: 6px;
    border: none;
    cursor: pointer;
    font-size: 0.9rem;
  }

  .btn-danger {
    background: #f85149;
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
