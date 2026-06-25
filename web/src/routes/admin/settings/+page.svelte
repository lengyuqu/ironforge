<script lang="ts">
  import { goto } from '$app/navigation';
  import { setBanner, clearBanner } from '$lib/stores/instance.svelte';
  import { isAuthReady, isLoggedIn, isAdmin } from '$lib/stores/auth.svelte';
  import {
    admin,
    type AdminSettings,
    type AdminSsoProvider,
    type SsoProviderPayload,
  } from '$lib/api/client.svelte';

  let loading = $state(true);
  let saving = $state(false);
  let maintenanceMode = $state(false);
  let bannerMessage = $state('');
  let bannerType = $state<'info' | 'warning' | 'error'>('info');
  let error = $state('');
  let ssoProviders = $state<AdminSsoProvider[]>([]);
  let ssoSaving = $state(false);
  let editingSsoId = $state<number | null>(null);
  let ssoForm = $state<SsoProviderPayload>(emptySsoProviderForm());

  function emptySsoProviderForm(): SsoProviderPayload {
    return {
      name: '',
      slug: '',
      provider_type: 'oauth2',
      client_id: '',
      client_secret: '',
      discovery_url: '',
      scopes: 'openid profile email',
      ldap_host: '',
      ldap_port: undefined,
      ldap_bind_dn: '',
      ldap_bind_password: '',
      ldap_base_dn: '',
      ldap_user_filter: '',
      enabled: true,
      icon_url: '',
    };
  }

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
      const [data, providers] = await Promise.all([
        admin.getSettings(),
        admin.listSsoProviders(),
      ]);
      maintenanceMode = data.maintenance_mode;
      bannerMessage = data.banner_message || '';
      bannerType = data.banner_type || 'info';
      ssoProviders = providers;

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

  function editSsoProvider(provider: AdminSsoProvider) {
    editingSsoId = provider.id;
    ssoForm = {
      name: provider.name,
      slug: provider.slug,
      provider_type: provider.provider_type || 'oauth2',
      client_id: provider.client_id || '',
      client_secret: '',
      discovery_url: provider.discovery_url || '',
      scopes: provider.scopes || '',
      ldap_host: provider.ldap_host || '',
      ldap_port: provider.ldap_port ?? undefined,
      ldap_bind_dn: provider.ldap_bind_dn || '',
      ldap_bind_password: '',
      ldap_base_dn: provider.ldap_base_dn || '',
      ldap_user_filter: provider.ldap_user_filter || '',
      enabled: provider.enabled,
      icon_url: provider.icon_url || '',
    };
  }

  function resetSsoForm() {
    editingSsoId = null;
    ssoForm = emptySsoProviderForm();
  }

  function cleanSsoPayload(): SsoProviderPayload {
    return {
      ...ssoForm,
      name: ssoForm.name.trim(),
      slug: ssoForm.slug.trim(),
      provider_type: ssoForm.provider_type || 'oauth2',
      client_id: ssoForm.client_id?.trim() || undefined,
      client_secret: ssoForm.client_secret || undefined,
      discovery_url: ssoForm.discovery_url?.trim() || undefined,
      scopes: ssoForm.scopes?.trim() || undefined,
      ldap_host: ssoForm.ldap_host?.trim() || undefined,
      ldap_port: ssoForm.ldap_port ? Number(ssoForm.ldap_port) : undefined,
      ldap_bind_dn: ssoForm.ldap_bind_dn?.trim() || undefined,
      ldap_bind_password: ssoForm.ldap_bind_password || undefined,
      ldap_base_dn: ssoForm.ldap_base_dn?.trim() || undefined,
      ldap_user_filter: ssoForm.ldap_user_filter?.trim() || undefined,
      icon_url: ssoForm.icon_url?.trim() || undefined,
    };
  }

  async function saveSsoProvider() {
    if (!ssoForm.name.trim() || !ssoForm.slug.trim()) {
      error = 'SSO provider name and slug are required';
      return;
    }

    try {
      ssoSaving = true;
      error = '';
      const payload = cleanSsoPayload();
      if (editingSsoId) {
        await admin.updateSsoProvider(editingSsoId, payload);
      } else {
        await admin.createSsoProvider(payload);
      }
      ssoProviders = await admin.listSsoProviders();
      resetSsoForm();
    } catch (e: any) {
      error = e.message;
    } finally {
      ssoSaving = false;
    }
  }

  async function toggleSsoProvider(provider: AdminSsoProvider) {
    try {
      error = '';
      await admin.updateSsoProvider(provider.id, {
        name: provider.name,
        slug: provider.slug,
        provider_type: provider.provider_type,
        client_id: provider.client_id || undefined,
        discovery_url: provider.discovery_url || undefined,
        scopes: provider.scopes || undefined,
        ldap_host: provider.ldap_host || undefined,
        ldap_port: provider.ldap_port || undefined,
        ldap_bind_dn: provider.ldap_bind_dn || undefined,
        ldap_base_dn: provider.ldap_base_dn || undefined,
        ldap_user_filter: provider.ldap_user_filter || undefined,
        icon_url: provider.icon_url || undefined,
        enabled: !provider.enabled,
      });
      ssoProviders = await admin.listSsoProviders();
    } catch (e: any) {
      error = e.message;
    }
  }

  async function deleteSsoProvider(provider: AdminSsoProvider) {
    if (!confirm(`Delete SSO provider "${provider.name}"?`)) return;
    try {
      error = '';
      await admin.deleteSsoProvider(provider.id);
      ssoProviders = await admin.listSsoProviders();
      if (editingSsoId === provider.id) resetSsoForm();
    } catch (e: any) {
      error = e.message;
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

    <div class="section">
      <h2>SSO Providers</h2>

      {#if ssoProviders.length === 0}
        <p class="text-secondary">No SSO providers configured.</p>
      {:else}
        <div class="provider-list">
          {#each ssoProviders as provider (provider.id)}
            <div class="provider-row">
              <div>
                <strong>{provider.name}</strong>
                <div class="provider-meta">
                  <span>{provider.slug}</span>
                  <span>{provider.provider_type}</span>
                  <span class:enabled={provider.enabled}>{provider.enabled ? 'Enabled' : 'Disabled'}</span>
                </div>
              </div>
              <div class="provider-actions">
                <button class="btn-secondary" type="button" onclick={() => toggleSsoProvider(provider)}>
                  {provider.enabled ? 'Disable' : 'Enable'}
                </button>
                <button class="btn-secondary" type="button" onclick={() => editSsoProvider(provider)}>Edit</button>
                <button class="btn-danger" type="button" onclick={() => deleteSsoProvider(provider)}>Delete</button>
              </div>
            </div>
          {/each}
        </div>
      {/if}

      <div class="sso-form">
        <h3>{editingSsoId ? 'Edit SSO Provider' : 'Add SSO Provider'}</h3>
        <div class="form-grid">
          <div class="form-group">
            <label for="sso-name">Name</label>
            <input id="sso-name" type="text" bind:value={ssoForm.name} placeholder="Google Workspace" />
          </div>
          <div class="form-group">
            <label for="sso-slug">Slug</label>
            <input id="sso-slug" type="text" bind:value={ssoForm.slug} placeholder="google" />
          </div>
          <div class="form-group">
            <label for="sso-type">Type</label>
            <select id="sso-type" bind:value={ssoForm.provider_type}>
              <option value="oauth2">OAuth2 / OIDC</option>
              <option value="ldap">LDAP</option>
            </select>
          </div>
          <div class="form-group">
            <label for="sso-client-id">Client ID</label>
            <input id="sso-client-id" type="text" bind:value={ssoForm.client_id} />
          </div>
          <div class="form-group">
            <label for="sso-client-secret">Client Secret</label>
            <input id="sso-client-secret" type="password" bind:value={ssoForm.client_secret} placeholder={editingSsoId ? 'Leave blank to keep existing secret' : ''} />
          </div>
          <div class="form-group">
            <label for="sso-discovery-url">Discovery URL</label>
            <input id="sso-discovery-url" type="url" bind:value={ssoForm.discovery_url} />
          </div>
          <div class="form-group">
            <label for="sso-scopes">Scopes</label>
            <input id="sso-scopes" type="text" bind:value={ssoForm.scopes} />
          </div>
          <div class="form-group">
            <label for="sso-icon-url">Icon URL</label>
            <input id="sso-icon-url" type="url" bind:value={ssoForm.icon_url} />
          </div>
          <div class="form-group">
            <label for="sso-ldap-host">LDAP Host</label>
            <input id="sso-ldap-host" type="text" bind:value={ssoForm.ldap_host} />
          </div>
          <div class="form-group">
            <label for="sso-ldap-port">LDAP Port</label>
            <input id="sso-ldap-port" type="number" min="1" bind:value={ssoForm.ldap_port} />
          </div>
          <div class="form-group">
            <label for="sso-ldap-bind-dn">LDAP Bind DN</label>
            <input id="sso-ldap-bind-dn" type="text" bind:value={ssoForm.ldap_bind_dn} />
          </div>
          <div class="form-group">
            <label for="sso-ldap-bind-password">LDAP Bind Password</label>
            <input id="sso-ldap-bind-password" type="password" bind:value={ssoForm.ldap_bind_password} placeholder={editingSsoId ? 'Leave blank to keep existing password' : ''} />
          </div>
          <div class="form-group">
            <label for="sso-ldap-base-dn">LDAP Base DN</label>
            <input id="sso-ldap-base-dn" type="text" bind:value={ssoForm.ldap_base_dn} />
          </div>
          <div class="form-group">
            <label for="sso-ldap-filter">LDAP User Filter</label>
            <input id="sso-ldap-filter" type="text" bind:value={ssoForm.ldap_user_filter} placeholder={'(uid={username})'} />
          </div>
        </div>
        <div class="toggle-row">
          <input id="sso-enabled" type="checkbox" bind:checked={ssoForm.enabled} />
          <label for="sso-enabled">Enable this provider</label>
        </div>
        <div class="inline-actions">
          <button class="btn-primary" type="button" onclick={saveSsoProvider} disabled={ssoSaving}>
            {ssoSaving ? 'Saving...' : editingSsoId ? 'Update Provider' : 'Create Provider'}
          </button>
          {#if editingSsoId}
            <button class="btn-secondary" type="button" onclick={resetSsoForm}>Cancel</button>
          {/if}
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .settings-page { max-width: 700px; margin: 0 auto; padding: 24px; }
  h1 { font-size: 22px; margin-bottom: 24px; }
  h2 { font-size: 16px; margin: 0 0 12px; }
  h3 { font-size: 14px; margin: 18px 0 12px; }
.section { background: var(--bg-secondary); border: 1px solid var(--border); border-radius: var(--radius); padding: 16px; margin-bottom: 16px; }
  .toggle-row { display: flex; align-items: center; gap: 10px; font-size: 14px; cursor: pointer; }
  .toggle-row input[type="checkbox"] { width: 18px; height: 18px; }
  .form-group { margin-top: 12px; }
  .form-group label { display: block; font-size: 13px; color: var(--text-secondary); margin-bottom: 4px; }
  .form-group input, .form-group select { width: 100%; padding: 8px 12px; border: 1px solid var(--border); border-radius: var(--radius); font-size: 14px; background: var(--bg-primary); color: var(--text-primary); box-sizing: border-box; }
  .actions { margin-top: 16px; }
  .inline-actions { display: flex; gap: 8px; margin-top: 16px; }
  .btn-primary { padding: 8px 20px; background: var(--accent); color: #fff; border: none; border-radius: var(--radius); font-size: 14px; cursor: pointer; }
  .btn-primary:disabled { opacity: 0.6; cursor: not-allowed; }
  .btn-secondary,
  .btn-danger {
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-primary);
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
  }
  .btn-danger { color: #cf222e; }
  .provider-list { display: flex; flex-direction: column; gap: 8px; margin-bottom: 16px; }
  .provider-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-primary);
  }
  .provider-meta { display: flex; flex-wrap: wrap; gap: 8px; color: var(--text-secondary); font-size: 12px; margin-top: 4px; }
  .provider-meta .enabled { color: #1a7f37; }
  .provider-actions { display: flex; flex-wrap: wrap; gap: 6px; justify-content: flex-end; }
  .form-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 0 12px; }
  @media (max-width: 720px) {
    .provider-row { align-items: stretch; flex-direction: column; }
    .provider-actions { justify-content: flex-start; }
    .form-grid { grid-template-columns: 1fr; }
  }
</style>
