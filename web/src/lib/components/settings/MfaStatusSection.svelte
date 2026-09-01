<script lang="ts">
  import { mfa, type MfaBackupStatus } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';

  let {
    backupStatus,
    saving = false,
    onStartSetup,
    onDisabled,
  }: {
    backupStatus: MfaBackupStatus | null;
    saving?: boolean;
    onStartSetup: () => void | Promise<void>;
    onDisabled: () => void | Promise<void>;
  } = $props();

  let disablePassword = $state('');
  let disabling = $state(false);

  const mfaEnabled = $derived((backupStatus?.total ?? 0) > 0);

  async function disableMfa(event: SubmitEvent) {
    event.preventDefault();
    if (!disablePassword) {
      toast.error('Current password is required');
      return;
    }
    if (!confirm('Disable multi-factor authentication for your account?')) return;

    disabling = true;
    try {
      await mfa.disable(disablePassword);
      disablePassword = '';
      toast.success('MFA disabled');
      await onDisabled();
    } catch (e) {
      toast.error(toErrorMessage(e, 'Disable failed'));
    } finally {
      disabling = false;
    }
  }
</script>

<section class="section">
  <div class="section-heading">
    <div>
      <h2>Multi-Factor Authentication</h2>
      <p>Add a time-based authenticator code after password login.</p>
    </div>
    <span class:enabled={mfaEnabled} class="status">{mfaEnabled ? 'Enabled' : 'Disabled'}</span>
  </div>

  {#if mfaEnabled}
    <div class="summary-grid">
      <div>
        <strong>{backupStatus?.unused ?? 0}</strong>
        <span>unused backup codes</span>
      </div>
      <div>
        <strong>{backupStatus?.total ?? 0}</strong>
        <span>total backup codes</span>
      </div>
    </div>

    <form class="disable-form" onsubmit={disableMfa}>
      <label>
        Current password
        <input type="password" bind:value={disablePassword} autocomplete="current-password" disabled={disabling || saving} />
      </label>
      <button type="submit" class="btn btn-danger" disabled={disabling || saving || !disablePassword}>
        {disabling ? 'Disabling...' : 'Disable MFA'}
      </button>
    </form>
  {:else}
    <p class="muted">MFA is not enabled for this account.</p>
    <button type="button" class="btn btn-primary" onclick={onStartSetup} disabled={saving}>
      {saving ? 'Starting...' : 'Set up MFA'}
    </button>
  {/if}
</section>

<style>
  .section {
    margin-bottom: 20px;
    padding: 20px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-secondary);
  }

  .section-heading {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 18px;
  }

  h2 {
    margin: 0;
    font-size: 18px;
  }

  p {
    margin: 0;
  }

  .muted,
  .section-heading p {
    color: var(--text-secondary);
  }

  .status {
    flex: 0 0 auto;
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 4px 10px;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 700;
  }

  .status.enabled {
    border-color: var(--green-dim);
    color: var(--green);
  }

  .summary-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
    margin-bottom: 18px;
  }

  .summary-grid > div {
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-primary);
  }

  .summary-grid strong {
    display: block;
    margin-bottom: 4px;
    font-size: 24px;
  }

  .summary-grid span {
    color: var(--text-secondary);
    font-size: 13px;
  }

  form {
    display: grid;
    gap: 12px;
    max-width: 420px;
  }

  label {
    display: grid;
    gap: 6px;
    font-size: 13px;
    font-weight: 600;
  }

  .btn {
    width: fit-content;
    padding: 8px 14px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    cursor: pointer;
    font-weight: 600;
  }

  .btn-primary {
    border-color: var(--green-dim);
    background: var(--green-dim);
    color: #fff;
  }

  .btn-danger {
    border-color: var(--red-dim);
    background: var(--red-dim);
    color: #fff;
  }

  .btn:disabled {
    cursor: not-allowed;
    opacity: 0.65;
  }

  @media (max-width: 720px) {
    .section-heading {
      display: grid;
      grid-template-columns: 1fr;
    }

    .summary-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
