<script lang="ts">
  import { goto } from '$app/navigation';
  import { setBanner, clearBanner } from '$lib/stores/instance.svelte';
  import { isAuthReady, isLoggedIn, isAdmin } from '$lib/stores/auth.svelte';
  import { admin, type AdminSettings } from '$lib/api/client.svelte';

  let loading = $state(true);
  let saving = $state(false);
  let maintenanceMode = $state(false);
  let bannerMessage = $state('');
  let bannerType = $state<'info' | 'warning' | 'error'>('info');
  let error = $state('');

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
      const data = await admin.getSettings();
      maintenanceMode = data.maintenance_mode;
      bannerMessage = data.banner_message || '';
      bannerType = data.banner_type || 'info';

      if (data.banner_message) {
        setBanner(data.banner_message, data.banner_type);
      } else {
        clearBanner();
      }
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function saveSettings() {
    try {
      saving = true;
      error = '';
      const payload: Partial<AdminSettings> = {
        maintenance_mode: maintenanceMode,
        banner_message: bannerMessage || null,
        banner_type: bannerType,
      };
      const data = await admin.updateSettings(payload);
      // Sync banner to frontend store
      if (data.banner_message) {
        setBanner(data.banner_message, data.banner_type);
      } else {
        clearBanner();
      }
    } catch (e: any) {
      error = e.message;
    } finally {
      saving = false;
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
  {:else}
    <div class="section">
      <h2>Maintenance Mode</h2>
      <div class="toggle-row">
        <input id="admin-maintenance-mode" type="checkbox" bind:checked={maintenanceMode} />
        <label for="admin-maintenance-mode">Enable maintenance mode (read-only, blocks all mutating requests)</label>
      </div>
    </div>

    <div class="section">
      <h2>Instance Banner</h2>
      <div class="form-group">
        <label for="admin-banner-message">Banner Message (leave empty to hide)</label>
        <input id="admin-banner-message" type="text" bind:value={bannerMessage} placeholder="e.g. Scheduled maintenance tonight at 2am" />
      </div>
      <div class="form-group">
        <label for="admin-banner-type">Banner Type</label>
        <select id="admin-banner-type" bind:value={bannerType}>
          <option value="info">Info (blue)</option>
          <option value="warning">Warning (yellow)</option>
          <option value="error">Error (red)</option>
        </select>
      </div>
    </div>

    <div class="actions">
      <button class="btn-primary" onclick={saveSettings} disabled={saving}>
        {saving ? 'Saving...' : 'Save Settings'}
      </button>
    </div>
  {/if}
</div>

<style>
  .settings-page { max-width: 700px; margin: 0 auto; padding: 24px; }
  h1 { font-size: 22px; margin-bottom: 24px; }
  h2 { font-size: 16px; margin: 0 0 12px; }
.section { background: var(--bg-secondary); border: 1px solid var(--border); border-radius: var(--radius); padding: 16px; margin-bottom: 16px; }
  .toggle-row { display: flex; align-items: center; gap: 10px; font-size: 14px; cursor: pointer; }
  .toggle-row input[type="checkbox"] { width: 18px; height: 18px; }
  .form-group { margin-top: 12px; }
  .form-group label { display: block; font-size: 13px; color: var(--text-secondary); margin-bottom: 4px; }
  .form-group input, .form-group select { width: 100%; padding: 8px 12px; border: 1px solid var(--border); border-radius: var(--radius); font-size: 14px; background: var(--bg-primary); color: var(--text-primary); box-sizing: border-box; }
  .actions { margin-top: 16px; }
  .btn-primary { padding: 8px 20px; background: var(--accent); color: #fff; border: none; border-radius: var(--radius); font-size: 14px; cursor: pointer; }
  .btn-primary:disabled { opacity: 0.6; cursor: not-allowed; }
</style>
