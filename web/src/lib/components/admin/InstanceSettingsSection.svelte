<script lang="ts">
  // Instance settings section — maintenance mode toggle + instance banner.
  // Self-contained: initialises from the loaded settings and persists via
  // admin.updateSettings, syncing the banner store on save.
  import { setBanner, clearBanner } from '$lib/stores/instance.svelte';
  import { admin } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';

  interface Props {
    initialMaintenanceMode: boolean;
    initialBannerMessage: string;
    initialBannerType: 'info' | 'warning' | 'error';
  }

  let {
    initialMaintenanceMode,
    initialBannerMessage,
    initialBannerType
  }: Props = $props();

  // Snapshot initial values (component mounts once per page load).
  let maintenanceMode = $state(initialMaintenanceMode);
  let bannerMessage = $state(initialBannerMessage);
  let bannerType = $state<'info' | 'warning' | 'error'>(initialBannerType);
  let saving = $state(false);

  async function saveSettings() {
    try {
      saving = true;
      const data = await admin.updateSettings({
        maintenance_mode: maintenanceMode,
        banner_message: bannerMessage || null,
        banner_type: bannerType,
      });
      // Sync banner to frontend store
      if (data.banner_message) {
        setBanner(data.banner_message, data.banner_type);
      } else {
        clearBanner();
      }
      toast.success('Settings saved');
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      saving = false;
    }
  }
</script>

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
    <input
      id="admin-banner-message"
      type="text"
      bind:value={bannerMessage}
      placeholder="e.g. Scheduled maintenance tonight at 2am"
    />
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

<style>
  h2 { font-size: 16px; margin: 0 0 12px; }

  .section {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px;
    margin-bottom: 16px;
  }

  .toggle-row { display: flex; align-items: center; gap: 10px; font-size: 14px; cursor: pointer; }
  .toggle-row input[type='checkbox'] { width: 18px; height: 18px; }

  .form-group { margin-top: 12px; }
  .form-group label { display: block; font-size: 13px; color: var(--text-secondary); margin-bottom: 4px; }
  .form-group input,
  .form-group select {
    width: 100%;
    padding: 8px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 14px;
    background: var(--bg-primary);
    color: var(--text-primary);
    box-sizing: border-box;
  }

  .actions { margin-top: 16px; }

  .btn-primary {
    padding: 8px 20px;
    background: var(--accent);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    font-size: 14px;
    cursor: pointer;
  }
  .btn-primary:disabled { opacity: 0.6; cursor: not-allowed; }
</style>
