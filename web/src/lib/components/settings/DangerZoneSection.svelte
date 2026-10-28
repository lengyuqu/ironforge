<script lang="ts">
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import { confirmDialog } from '$lib/components/confirm.svelte';
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

    const confirmed = (await confirmDialog({ variant: 'danger', message: t('settings.delete.desc') }));
    if (!confirmed) return;

    try {
      deleting = true;
      deleteError = '';

      await repos.delete(owner, repo);

      // Redirect to dashboard
      goto('/dashboard');
    } catch (err) {
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
      <ErrorBanner message={deleteError} />
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
    margin-bottom: 1rem;
    color: var(--text-primary);
  }

  .section {
    border-bottom: none;
  }

  .danger-box {
    border: 1px solid var(--red);
    background: rgba(255, 0, 0, 0.05);
    border-radius: 6px;
    padding: 1.5rem;
  }

  .danger-box h3 {
    color: var(--red);
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

  input[type='text'] {
    width: 100%;
  }

</style>
