<script lang="ts">
  import { mfa, type MfaSetupResponse } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    setup,
    onEnabled,
  }: {
    setup: MfaSetupResponse;
    onEnabled: (backupCodes: string[]) => void | Promise<void>;
  } = $props();

  let verificationCode = $state('');
  let enabling = $state(false);

  async function enableMfa(event: SubmitEvent) {
    event.preventDefault();
    if (!verificationCode.trim()) {
      toast.error(t('settings.mfa.code_required'));
      return;
    }

    enabling = true;
    try {
      const result = await mfa.enable(verificationCode.trim());
      await onEnabled(result.backup_codes);
    } catch (e) {
      toast.error(toErrorMessage(e, t('settings.mfa.enable_failed')));
    } finally {
      enabling = false;
    }
  }
</script>

<section class="section setup-section surface">
  <h2>{t('settings.mfa.scan_title')}</h2>
  <div class="setup-grid">
    <div class="qr" aria-label={t('settings.mfa.qr_label')}>{@html setup.qr_svg}</div>
    <div>
      <p class="muted">{t('settings.mfa.scan_desc')}</p>
      <code>{setup.secret}</code>
      <form class="enable-form" onsubmit={enableMfa}>
        <label>
          {t('auth.login.mfa_code')}
          <input inputmode="numeric" autocomplete="one-time-code" bind:value={verificationCode} disabled={enabling} />
        </label>
        <button type="submit" class="btn btn-primary" disabled={enabling || !verificationCode.trim()}>
          {enabling ? t('auth.login.verifying') : t('settings.mfa.enable')}
        </button>
      </form>
    </div>
  </div>
</section>

<style>
  .section {
    margin-bottom: 20px;
    padding: 20px;
  }

  h2 {
    margin: 0;
  }

  p {
    margin: 0;
  }

  .muted {
    color: var(--text-secondary);
  }

  .setup-grid {
    display: grid;
    grid-template-columns: 220px minmax(0, 1fr);
    gap: 20px;
    align-items: start;
    margin-top: 16px;
  }

  .qr {
    display: grid;
    place-items: center;
    min-height: 220px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: #fff;
    color: #111;
  }

  .qr :global(svg) {
    width: 196px;
    height: 196px;
  }

  .setup-section code {
    display: block;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-primary);
    margin-top: 8px;
  }

  form {
    display: grid;
    gap: 12px;
    max-width: 420px;
  }

  .enable-form {
    margin-top: 16px;
  }

  label {
    display: grid;
    gap: 6px;
    font-size: 13px;
    font-weight: 600;
  }

  @media (max-width: 720px) {
    .setup-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
