<script lang="ts">
  import { createT } from '$lib/i18n';
  import { confirmDialog } from '$lib/components/confirm.svelte';
  import { mfa, type MfaBackupStatus } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';


  const t = createT();
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
      toast.error(t('settings.mfa.password_required'));
      return;
    }
    if (!(await confirmDialog({ variant: 'danger', message: t('confirm.mfa_disable') }))) return;

    disabling = true;
    try {
      await mfa.disable(disablePassword);
      disablePassword = '';
      toast.success(t('settings.mfa.disabled_toast'));
      await onDisabled();
    } catch (e) {
      toast.error(toErrorMessage(e, t('settings.mfa.disable_failed')));
    } finally {
      disabling = false;
    }
  }
</script>

<section class="section surface">
  <div class="section-heading">
    <div>
      <h2>{t('settings.mfa.title')}</h2>
      <p>{t('settings.mfa.desc')}</p>
    </div>
    <span class:enabled={mfaEnabled} class="status">{mfaEnabled ? t('settings.mfa.status_enabled') : t('settings.mfa.status_disabled')}</span>
  </div>

  {#if mfaEnabled}
    <div class="summary-grid">
      <div>
        <strong>{backupStatus?.unused ?? 0}</strong>
        <span>{t('settings.mfa.unused_codes')}</span>
      </div>
      <div>
        <strong>{backupStatus?.total ?? 0}</strong>
        <span>{t('settings.mfa.total_codes')}</span>
      </div>
    </div>

    <form class="disable-form" onsubmit={disableMfa}>
      <label>
        {t('settings.mfa.current_password')}
        <input type="password" bind:value={disablePassword} autocomplete="current-password" disabled={disabling || saving} />
      </label>
      <button type="submit" class="btn btn-danger" disabled={disabling || saving || !disablePassword}>
        {disabling ? t('settings.mfa.disabling') : t('settings.mfa.disable')}
      </button>
    </form>
  {:else}
    <p class="muted">{t('settings.mfa.not_enabled')}</p>
    <button type="button" class="btn btn-primary" onclick={onStartSetup} disabled={saving}>
      {saving ? t('settings.mfa.starting') : t('settings.mfa.setup')}
    </button>
  {/if}
</section>

<style>
  .section {
    margin-bottom: 20px;
    padding: 20px;
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
