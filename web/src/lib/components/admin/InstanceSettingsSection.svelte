<script lang="ts">
  // Instance settings section — maintenance mode toggle + instance banner.
  // Self-contained: initialises from the loaded settings and persists via
  // admin.updateSettings, syncing the banner store on save.
  import FormField from '$lib/components/FormField.svelte';
  import { setBanner, clearBanner } from '$lib/stores/instance.svelte';
  import { admin } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

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
      toast.success(t('admin.instance.saved'));
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      saving = false;
    }
  }
</script>

<div class="section surface">
  <h2>{t('admin.instance.maintenance_title')}</h2>
  <div class="toggle-row">
    <input id="admin-maintenance-mode" type="checkbox" bind:checked={maintenanceMode} />
    <label for="admin-maintenance-mode">{t('admin.instance.maintenance_enable')}</label>
  </div>
</div>

<div class="section surface">
  <h2>{t('admin.instance.banner_title')}</h2>
  <FormField id="admin-banner-message" label={t('admin.instance.banner_message')}>
    {#snippet children()}
      <input
        id="admin-banner-message"
        type="text"
        bind:value={bannerMessage}
        placeholder={t('admin.instance.banner_placeholder')}
      />
    {/snippet}
  </FormField>
  <FormField id="admin-banner-type" label={t('admin.instance.banner_type')}>
    {#snippet children()}
      <select id="admin-banner-type" bind:value={bannerType}>
        <option value="info">{t('admin.instance.banner_info')}</option>
        <option value="warning">{t('admin.instance.banner_warning')}</option>
        <option value="error">{t('admin.instance.banner_error')}</option>
      </select>
    {/snippet}
  </FormField>
</div>

<div class="actions">
  <button class="btn-primary" onclick={saveSettings} disabled={saving}>
    {saving ? 'Saving...' : 'Save Settings'}
  </button>
</div>

<style>
  h2 {
    margin: 0 0 12px;
  }

  .section {
    padding: 16px;
    margin-bottom: 16px;
  }

  .toggle-row { display: flex; align-items: center; gap: 10px; font-size: 14px; cursor: pointer; }
  .toggle-row input[type='checkbox'] { width: 18px; height: 18px; }

  .actions { margin-top: 16px; }
</style>
