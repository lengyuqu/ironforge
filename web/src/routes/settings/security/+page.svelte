<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import SuccessBanner from '$lib/components/SuccessBanner.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
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
    } catch (err) {
      error = toErrorMessage(err, t('errors.load_failed'));
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
    } catch (err) {
      error = toErrorMessage(err, t('errors.start_failed'));
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
  <title>{t('settings.security.title')} · IronForge</title>
</svelte:head>

<div class="page-container security-page">
  <header class="page-header">
    <div>
      <h1>{t('settings.security.title')}</h1>
      <p>{t('settings.security.desc')}</p>
    </div>
  </header>

  {#if error}
    <ErrorBanner message={error} />
  {/if}

  {#if success}
    <SuccessBanner message={success} />
  {/if}

  {#if loading}
    <LoadingState />
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

  h1 {
    margin: 0 0 6px;
  }

  p {
    margin: 0;
  }

  .page-header p {
    color: var(--text-secondary);
  }
</style>
