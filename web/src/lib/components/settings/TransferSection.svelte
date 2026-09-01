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

  let newOwner = $state('');
  let transferring = $state(false);
  let transferError = $state('');
  let transferSuccess = $state('');

  async function handleTransfer() {
    if (!newOwner.trim()) return;

    const confirmed = confirm(t('settings.transfer.warning'));
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
    } catch (err: any) {
      transferError = toErrorMessage(err, 'Transfer failed');
    } finally {
      transferring = false;
    }
  }
</script>

<section class="section transfer-section">
  <h2>{t('settings.transfer.title')}</h2>
  <p class="section-desc">{t('settings.transfer.desc')}</p>

  <div class="warning-box">
    <span class="warning-icon">⚠️</span>
    <p>{t('settings.transfer.warning')}</p>
  </div>

  {#if transferError}
    <div class="error-box">{transferError}</div>
  {/if}

  {#if transferSuccess}
    <div class="success-box">{transferSuccess}</div>
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
    font-size: 1.25rem;
    margin-bottom: 1rem;
    color: var(--text-primary);
  }

  .section {
    margin-bottom: 2.5rem;
    padding-bottom: 2rem;
    border-bottom: 1px solid var(--border);
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
    border: 1px solid var(--orange, #ff8800);
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

  .form-group label {
    display: block;
    margin-bottom: 0.5rem;
    color: var(--text-primary);
    font-weight: 500;
    font-size: 0.9rem;
  }

  .input-row {
    display: flex;
    gap: 0.75rem;
  }

  input[type='text'] {
    flex: 1;
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
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-warning {
    background: var(--orange, #ff8800);
    color: white;
  }

  .btn-warning:hover:not(:disabled) {
    background: var(--orange-dark, #cc6600);
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

  .success-box {
    padding: 0.75rem;
    background: rgba(0, 255, 0, 0.1);
    border: 1px solid var(--green, #28a745);
    border-radius: 6px;
    color: var(--green, #28a745);
    font-size: 0.9rem;
    margin-bottom: 1rem;
  }
</style>
