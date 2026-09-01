<script lang="ts">
  // Admin instance settings page — orchestration layer: auth guard + initial
  // data load (settings / SSO providers / first login-attempts page); the
  // three sections manage their own state and API calls.
  import { goto } from '$app/navigation';
  import { setBanner, clearBanner } from '$lib/stores/instance.svelte';
  import { isAuthReady, isLoggedIn, isAdmin } from '$lib/stores/auth.svelte';
  import { admin, type AdminSsoProvider, type LoginAttemptEntry } from '$lib/api/client.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import InstanceSettingsSection from '$lib/components/admin/InstanceSettingsSection.svelte';
  import SsoProviderSection from '$lib/components/admin/SsoProviderSection.svelte';
  import LoginAttemptsSection from '$lib/components/admin/LoginAttemptsSection.svelte';

  let loading = $state(true);
  let error = $state('');
  let settings = $state<Awaited<ReturnType<typeof admin.getSettings>> | null>(null);
  let ssoProviders = $state<AdminSsoProvider[]>([]);
  let loginAttempts = $state<LoginAttemptEntry[]>([]);
  let loginAttemptsTotal = $state(0);
  let loginAttemptsPage = $state(1);

  $effect(() => {
    if (!isAuthReady()) return;
    if (!isLoggedIn()) {
      goto('/login');
      return;
    }
    if (!isAdmin()) {
      goto('/dashboard');
      return;
    }
    loadSettings();
  });

  async function loadSettings() {
    try {
      loading = true;
      const [data, providers, loginAttemptResult] = await Promise.all([
        admin.getSettings(),
        admin.listSsoProviders(),
        admin.listLoginAttempts({ page: 1, per_page: 20 }),
      ]);
      settings = data;
      ssoProviders = providers;
      loginAttempts = loginAttemptResult.attempts;
      loginAttemptsTotal = loginAttemptResult.total;
      loginAttemptsPage = loginAttemptResult.page;

      if (data.banner_message) {
        setBanner(data.banner_message, data.banner_type);
      } else {
        clearBanner();
      }
    } catch (e: unknown) {
      error = toErrorMessage(e, 'Load failed');
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head>
  <title>Instance Settings · Admin · IronForge</title>
</svelte:head>

<div class="settings-page">
  <h1>Instance Settings</h1>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">Loading...</p>
  {:else if settings}
    <InstanceSettingsSection
      initialMaintenanceMode={settings.maintenance_mode}
      initialBannerMessage={settings.banner_message || ''}
      initialBannerType={settings.banner_type || 'info'}
    />

    <SsoProviderSection initialProviders={ssoProviders} />

    <LoginAttemptsSection
      initialAttempts={loginAttempts}
      initialTotal={loginAttemptsTotal}
      initialPage={loginAttemptsPage}
    />
  {/if}
</div>

<style>
  .settings-page { max-width: 700px; margin: 0 auto; padding: 24px; }
  h1 { font-size: 22px; margin-bottom: 24px; }
</style>
