<script lang="ts">
  import { mfa, type MfaSetupResponse } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';

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
      toast.error('Authentication code is required');
      return;
    }

    enabling = true;
    try {
      const result = await mfa.enable(verificationCode.trim());
      await onEnabled(result.backup_codes);
    } catch (e) {
      toast.error(toErrorMessage(e, 'Enable failed'));
    } finally {
      enabling = false;
    }
  }
</script>

<section class="section setup-section">
  <h2>Scan Authenticator QR</h2>
  <div class="setup-grid">
    <div class="qr" aria-label="Authenticator QR code">{@html setup.qr_svg}</div>
    <div>
      <p class="muted">Scan the QR code with an authenticator app, then enter the six-digit code.</p>
      <code>{setup.secret}</code>
      <form class="enable-form" onsubmit={enableMfa}>
        <label>
          Authentication code
          <input inputmode="numeric" autocomplete="one-time-code" bind:value={verificationCode} disabled={enabling} />
        </label>
        <button type="submit" class="btn btn-primary" disabled={enabling || !verificationCode.trim()}>
          {enabling ? 'Verifying...' : 'Enable MFA'}
        </button>
      </form>
    </div>
  </div>
</section>

<style>
  .section {
    margin-bottom: 20px;
    padding: 20px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-secondary);
  }

  h2 {
    margin: 0;
    font-size: 18px;
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

  .btn:disabled {
    cursor: not-allowed;
    opacity: 0.65;
  }

  @media (max-width: 720px) {
    .setup-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
