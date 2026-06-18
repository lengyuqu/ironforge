<script lang="ts">
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { createT } from '$lib/i18n';
  import { setBanner } from '$lib/stores/instance.svelte';

  const t = createT();

  let loading = $state(true);
  let saving = $state(false);
  let maintenanceMode = $state(false);
  let bannerMessage = $state('');
  let bannerType = $state('info');
  let error = $state('');

  $effect(() => { loadSettings(); });

  async function loadSettings() {
    try {
      loading = true;
      const res = await fetch('/api/v1/admin/settings', {
        headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
      });
      if (!res.ok) throw new Error('Failed to load settings');
      const data = await res.json();
      maintenanceMode = data.maintenance_mode;
      bannerMessage = data.banner_message || '';
      bannerType = data.banner_type || 'info';
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
      const res = await fetch('/api/v1/admin/settings', {
        method: 'PATCH',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${localStorage.getItem('token')}`,
        },
        body: JSON.stringify({
          maintenance_mode: maintenanceMode,
          banner_message: bannerMessage || null,
          banner_type: bannerType,
        }),
      });
      if (!res.ok) throw new Error('Failed to save settings');
      const data = await res.json();
      // Sync banner to frontend store
      if (data.banner_message) {
        setBanner(data.banner_message, data.banner_type as any);
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
      <label class="toggle-row">
        <input type="checkbox" bind:checked={maintenanceMode} />
        <span>Enable maintenance mode (read-only, blocks all mutating requests)</span>
      </label>
    </div>

    <div class="section">
      <h2>Instance Banner</h2>
      <div class="form-group">
        <label>Banner Message (leave empty to hide)</label>
        <input type="text" bind:value={bannerMessage} placeholder="e.g. Scheduled maintenance tonight at 2am" />
      </div>
      <div class="form-group">
        <label>Banner Type</label>
        <select bind:value={bannerType}>
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
  .error-banner { background: rgba(248,81,73,0.1); border: 1px solid var(--red-dim); color: var(--red); border-radius: var(--radius); padding: 10px 14px; font-size: 13px; margin-bottom: 16px; }
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
