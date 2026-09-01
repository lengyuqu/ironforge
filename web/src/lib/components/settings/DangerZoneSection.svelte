<script lang="ts">
  import { goto } from '$app/navigation';
  import { repos } from '$lib/api/client.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    repo,
  }: {
    owner: string;
    repo: string;
  } = $props();

  const repositoryPath = $derived(`${owner}/${repo}`);

  let deleteConfirm = $state('');
  let deleting = $state(false);
  let deleteError = $state('');

  async function handleDelete() {
    if (deleteConfirm !== repositoryPath) return;

    const confirmed = confirm(t('settings.delete.desc'));
    if (!confirmed) return;

    try {
      deleting = true;
      deleteError = '';

      await repos.delete(owner, repo);

      // Redirect to dashboard
      goto('/dashboard');
    } catch (err: any) {
      deleteError = toErrorMessage(err, 'Delete failed');
      deleting = false;
    }
  }
</script>

<section class="section danger-zone">
  <h2>{t('settings.danger_zone')}</h2>

  <div class="danger-box">
    <h3>{t('settings.delete.title')}</h3>
    <p>{t('settings.delete.desc')}</p>

    {#if deleteError}
      <div class="error-box">{deleteError}</div>
    {/if}

    <div class="form-group">
      <label for="delete-confirm"
        >{@html t('settings.delete.confirm_instruction', { repo: repositoryPath })}</label
      >
      <input
        id="delete-confirm"
        type="text"
        bind:value={deleteConfirm}
        placeholder={t('settings.delete.confirm_placeholder')}
        disabled={deleting}
      />
    </div>

    <button
      class="btn btn-danger"
      onclick={handleDelete}
      disabled={deleteConfirm !== repositoryPath || deleting}
    >
      {deleting ? t('settings.delete.confirming') : t('settings.delete.confirm_button')}
    </button>
  </div>
</section>

<style>
  h2 {
    font-size: 1.25rem;
    margin-bottom: 1rem;
    color: var(--text-primary);
  }

  .section {
    border-bottom: none;
  }

  .danger-box {
    border: 1px solid var(--red, #ff4444);
    background: rgba(255, 0, 0, 0.05);
    border-radius: 6px;
    padding: 1.5rem;
  }

  .danger-box h3 {
    color: var(--red, #ff4444);
    margin-bottom: 0.5rem;
    font-size: 1.1rem;
  }

  .danger-box p {
    color: var(--text-secondary);
    font-size: 0.9rem;
    margin-bottom: 1rem;
  }

  .form-group {
    margin-top: 1.5rem;
  }

  .form-group label {
    display: block;
    margin-bottom: 0.5rem;
    color: var(--text-primary);
    font-weight: 500;
    font-size: 0.9rem;
  }

  input[type='text'] {
    width: 100%;
    padding: 0.6rem 0.75rem;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 0.9rem;
  }

  input[type='text']:focus {
    outline: none;
    border-color: var(--accent);
  }

  .btn {
    padding: 0.6rem 1.25rem;
    border: none;
    border-radius: 6px;
    font-size: 0.9rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.2s;
    margin-top: 1rem;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-danger {
    background: var(--red, #ff4444);
    color: white;
  }

  .btn-danger:hover:not(:disabled) {
    background: var(--red-dark, #cc0000);
  }

  .error-box {
    padding: 0.75rem;
    background: rgba(255, 0, 0, 0.1);
    border: 1px solid var(--red, #ff4444);
    border-radius: 6px;
    color: var(--red, #ff4444);
    font-size: 0.9rem;
    margin-bottom: 1rem;
  }
</style>
