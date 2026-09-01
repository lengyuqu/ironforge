<script lang="ts">
  import { goto } from '$app/navigation';
  import { isAuthReady, isLoggedIn } from '$lib/stores/auth.svelte';
  import { mfa, type MfaBackupStatus, type MfaSetupResponse } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import MfaStatusSection from '$lib/components/settings/MfaStatusSection.svelte';
  import MfaSetupPanel from '$lib/components/settings/MfaSetupPanel.svelte';
  import BackupCodesPanel from '$lib/components/settings/BackupCodesPanel.svelte';

  const t = createT();

  let loading = $state(true);
  let saving = $state(false);
  let error = $state('');
  let success = $state('');
  let backupStatus = $state<MfaBackupStatus | null>(null);
  let setup = $state<MfaSetupResponse | null>(null);
  let newBackupCodes = $state<string[]>([]);

  $effect(() => {
    if (!isAuthReady()) return;
    if (!isLoggedIn()) {
      goto('/login');
      return;
    }
    loadSecurity();
  });

  async function loadSecurity() {
    try {
      loading = true;
      error = '';
      backupStatus = await mfa.backup();
    } catch (err: any) {
      error = err.message || t('errors.load_failed');
    } finally {
      loading = false;
    }
  }

  async function startSetup() {
    try {
      saving = true;
      error = '';
      success = '';
      newBackupCodes = [];
      setup = await mfa.setup();
    } catch (err: any) {
      error = err.message || t('errors.start_failed');
    } finally {
      saving = false;
    }
  }

  async function handleEnabled(backupCodes: string[]) {
    newBackupCodes = backupCodes;
    setup = null;
    success = 'MFA enabled. Save your backup codes before leaving this page.';
    await loadSecurity();
  }

  async function handleDisabled() {
    newBackupCodes = [];
    setup = null;
    await loadSecurity();
  }
</script>

<svelte:head>
  <title>Security · IronForge</title>
</svelte:head>

<div class="page-container security-page">
  <header class="page-header">
    <div>
      <h1>Security</h1>
      <p>Manage account protections for web login and Git/API access.</p>
    </div>
  </header>

  {#if error}
    <div class="error-box">{error}</div>
  {/if}

  {#if success}
    <div class="success-box">{success}</div>
  {/if}

  {#if loading}
    <p class="muted">Loading...</p>
  {:else}
    <MfaStatusSection
      backupStatus={backupStatus}
      {saving}
      onStartSetup={startSetup}
      onDisabled={handleDisabled}
    />

    {#if setup}
      <MfaSetupPanel setup={setup} onEnabled={handleEnabled} />
    {/if}

    {#if newBackupCodes.length > 0}
      <BackupCodesPanel codes={newBackupCodes} />
    {/if}
  {/if}
</div>

<style>
  .security-page {
    max-width: 980px;
  }

  .page-header {
    margin-bottom: 24px;
  }

  h1 {
    margin: 0 0 6px;
    font-size: 28px;
  }

  p {
    margin: 0;
  }

  .page-header p,
  .muted {
    color: var(--text-secondary);
  }

  .error-box,
  .success-box {
    margin-bottom: 16px;
    padding: 12px 14px;
    border-radius: var(--radius);
  }

  .error-box {
    border: 1px solid var(--red-dim);
    background: color-mix(in srgb, var(--red-dim) 14%, transparent);
    color: var(--red);
  }

  .success-box {
    border: 1px solid var(--green-dim);
    background: color-mix(in srgb, var(--green-dim) 14%, transparent);
    color: var(--green);
  }
</style>
