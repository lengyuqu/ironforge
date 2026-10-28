<script lang="ts">
  import SuccessBanner from '$lib/components/SuccessBanner.svelte';
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

  let newOwner = $state('');
  let transferring = $state(false);
  let transferError = $state('');
  let transferSuccess = $state('');

  async function handleTransfer() {
    if (!newOwner.trim()) return;

    const confirmed = (await confirmDialog({ variant: 'danger', message: t('settings.transfer.warning') }));
    if (!confirmed) return;

    try {
      transferring = true;
      transferError = '';
      transferSuccess = '';

      await repos.transfer(owner, repo, newOwner.trim());
      transferSuccess = t('settings.transfer.success');
      // Redirect to new repo URL
      setTimeout(() => {
        goto(`/${newOwner.trim()}/${repo}`);
      }, 1500);
    } catch (err) {
      transferError = toErrorMessage(err, t('settings.transfer.failed'));
    } finally {
      transferring = false;
    }
  }
</script>

<section class="settings-section transfer-section">
  <h2>{t('settings.transfer.title')}</h2>
  <p class="section-desc">{t('settings.transfer.desc')}</p>

  <div class="warning-box">
    <span class="warning-icon">⚠️</span>
    <p>{t('settings.transfer.warning')}</p>
  </div>

  {#if transferError}
    <ErrorBanner message={transferError} />
  {/if}

  {#if transferSuccess}
    <SuccessBanner message={transferSuccess} />
  {/if}

  <div class="form-group">
    <label for="new-owner">{t('settings.transfer.new_owner')}</label>
    <div class="input-row">
      <input
        id="new-owner"
        type="text"
        bind:value={newOwner}
        placeholder={t('settings.transfer.new_owner_placeholder')}
        disabled={transferring}
      />
      <button
        class="btn btn-warning"
        onclick={handleTransfer}
        disabled={!newOwner.trim() || transferring}
      >
        {transferring ? t('settings.transfer.confirming') : t('settings.transfer.confirm')}
      </button>
    </div>
  </div>
</section>

<style>
  h2 {
    margin-bottom: 1rem;
    color: var(--text-primary);
  }

  .section-desc {
    color: var(--text-secondary);
    margin-bottom: 1.5rem;
    font-size: 0.9rem;
  }

  .warning-box {
    display: flex;
    gap: 0.75rem;
    padding: 1rem;
    background: rgba(255, 165, 0, 0.1);
    border: 1px solid var(--orange);
    border-radius: 6px;
    margin-bottom: 1.5rem;
  }

  .warning-icon {
    font-size: 1.25rem;
    flex-shrink: 0;
  }

  .warning-box p {
    color: var(--text-primary);
    font-size: 0.9rem;
    margin: 0;
  }

  .form-group {
    margin-top: 1.5rem;
  }

  .input-row {
    display: flex;
    gap: 0.75rem;
  }

  input[type='text'] {
    flex: 1;
  }

  .btn-warning {
    background: var(--orange);
    color: white;
  }

  .btn-warning:hover:not(:disabled) {
    background: var(--orange);
  }

</style>
