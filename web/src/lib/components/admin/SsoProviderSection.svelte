<script lang="ts">
  // SSO providers section — self-contained: loads/manages the provider list
  // and the create/edit form (test connection, enable/disable, delete).
  import FormField from '$lib/components/FormField.svelte';
  import { createT } from '$lib/i18n';
  import { confirmDialog } from '$lib/components/confirm.svelte';
  import { admin, type AdminSsoProvider, type SsoProviderPayload } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';


  const t = createT();
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

  interface Props {
    initialProviders: AdminSsoProvider[];
  }

  let { initialProviders }: Props = $props();

  let providers = $state<AdminSsoProvider[]>([]);
  let ssoSaving = $state(false);
  let testingSsoId = $state<number | null>(null);
  let ssoTestResult = $state<{ ok: boolean; message: string } | null>(null);
  let editingSsoId = $state<number | null>(null);
  let ssoForm = $state<SsoProviderPayload>(emptySsoProviderForm());

  // Snapshot the initial list (component mounts once per page load).
  providers = initialProviders;

  async function reloadProviders() {
    providers = await admin.listSsoProviders();
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
      toast.error(t('admin.sso.name_slug_required'));
      return;
    }

    try {
      ssoSaving = true;
      const payload = cleanSsoPayload();
      if (editingSsoId) {
        await admin.updateSsoProvider(editingSsoId, payload);
        toast.success(t('admin.sso.updated'));
      } else {
        await admin.createSsoProvider(payload);
        toast.success(t('admin.sso.created'));
      }
      await reloadProviders();
      resetSsoForm();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      ssoSaving = false;
    }
  }

  async function toggleSsoProvider(provider: AdminSsoProvider) {
    try {
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
      await reloadProviders();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    }
  }

  async function deleteSsoProvider(provider: AdminSsoProvider) {
    if (!(await confirmDialog({ variant: 'danger', message: t('confirm.sso_delete', { name: provider.name }) }))) return;
    try {
      await admin.deleteSsoProvider(provider.id);
      await reloadProviders();
      if (editingSsoId === provider.id) resetSsoForm();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    }
  }

  async function testSsoProvider(provider: AdminSsoProvider) {
    try {
      testingSsoId = provider.id;
      ssoTestResult = null;
      ssoTestResult = await admin.testSsoProvider(provider.id);
    } catch (e: unknown) {
      ssoTestResult = { ok: false, message: toErrorMessage(e, t('admin.sso.ldap_test_failed')) };
    } finally {
      testingSsoId = null;
    }
  }
</script>

<div class="section surface">
  <h2>{t('admin.sso.title')}</h2>
  {#if ssoTestResult}
    <div class="connection-result" class:success={ssoTestResult.ok}>
      {ssoTestResult.message}
    </div>
  {/if}

  {#if providers.length === 0}
    <p class="text-secondary">{t('admin.sso.empty')}</p>
  {:else}
    <div class="provider-list">
      {#each providers as provider (provider.id)}
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
            {#if provider.provider_type === 'ldap'}
              <button
                class="btn-secondary"
                type="button"
                disabled={testingSsoId === provider.id}
                onclick={() => testSsoProvider(provider)}
              >
                {testingSsoId === provider.id ? t('admin.sso.testing') : t('admin.sso.test_connection')}
              </button>
            {/if}
            <button class="btn-secondary" type="button" onclick={() => toggleSsoProvider(provider)}>
              {provider.enabled ? t('admin.sso.disable') : t('admin.sso.enable')}
            </button>
            <button class="btn-secondary" type="button" onclick={() => editSsoProvider(provider)}>{t('common.edit')}</button>
            <button class="btn-danger" type="button" onclick={() => deleteSsoProvider(provider)}>{t('common.delete')}</button>
          </div>
        </div>
      {/each}
    </div>
  {/if}

  <div class="sso-form">
    <h3>{editingSsoId ? t('admin.sso.edit_title') : t('admin.sso.add_title')}</h3>
    <div class="form-grid">
      <FormField id="sso-name" label={t('admin.sso.field.name')}>
        {#snippet children()}
          <input id="sso-name" type="text" bind:value={ssoForm.name} placeholder={t('admin.sso.name_placeholder')} />
        {/snippet}
      </FormField>
      <FormField id="sso-slug" label={t('admin.sso.field.slug')}>
        {#snippet children()}
          <input id="sso-slug" type="text" bind:value={ssoForm.slug} placeholder="google" />
        {/snippet}
      </FormField>
      <FormField id="sso-type" label={t('admin.sso.field.type')}>
        {#snippet children()}
          <select id="sso-type" bind:value={ssoForm.provider_type}>
            <option value="oauth2">{t('admin.sso.type_oauth2')}</option>
            <option value="ldap">LDAP</option>
          </select>
        {/snippet}
      </FormField>
      <FormField id="sso-client-id" label={t('admin.sso.field.client_id')}>
        {#snippet children()}
          <input id="sso-client-id" type="text" bind:value={ssoForm.client_id} />
        {/snippet}
      </FormField>
      <FormField id="sso-client-secret" label={t('admin.sso.field.client_secret')}>
        {#snippet children()}
          <input
            id="sso-client-secret"
            type="password"
            bind:value={ssoForm.client_secret}
            placeholder={editingSsoId ? t('admin.sso.keep_secret') : ''}
          />
        {/snippet}
      </FormField>
      <FormField id="sso-discovery-url" label={t('admin.sso.field.discovery_url')}>
        {#snippet children()}
          <input id="sso-discovery-url" type="url" bind:value={ssoForm.discovery_url} />
        {/snippet}
      </FormField>
      <FormField id="sso-scopes" label={t('admin.sso.field.scopes')}>
        {#snippet children()}
          <input id="sso-scopes" type="text" bind:value={ssoForm.scopes} />
        {/snippet}
      </FormField>
      <FormField id="sso-icon-url" label={t('admin.sso.field.icon_url')}>
        {#snippet children()}
          <input id="sso-icon-url" type="url" bind:value={ssoForm.icon_url} />
        {/snippet}
      </FormField>
      <FormField id="sso-ldap-host" label={t('admin.sso.field.ldap_host')}>
        {#snippet children()}
          <input id="sso-ldap-host" type="text" bind:value={ssoForm.ldap_host} placeholder={t('admin.sso.ldap_host_placeholder')} />
        {/snippet}
      </FormField>
      <FormField id="sso-ldap-port" label={t('admin.sso.field.ldap_port')}>
        {#snippet children()}
          <input id="sso-ldap-port" type="number" min="1" bind:value={ssoForm.ldap_port} />
        {/snippet}
      </FormField>
      <FormField id="sso-ldap-bind-dn" label={t('admin.sso.field.ldap_bind_dn')}>
        {#snippet children()}
          <input id="sso-ldap-bind-dn" type="text" bind:value={ssoForm.ldap_bind_dn} />
        {/snippet}
      </FormField>
      <FormField id="sso-ldap-bind-password" label={t('admin.sso.field.ldap_bind_password')}>
        {#snippet children()}
          <input
            id="sso-ldap-bind-password"
            type="password"
            bind:value={ssoForm.ldap_bind_password}
            placeholder={editingSsoId ? t('admin.sso.keep_password') : ''}
          />
        {/snippet}
      </FormField>
      <FormField id="sso-ldap-base-dn" label={t('admin.sso.field.ldap_base_dn')}>
        {#snippet children()}
          <input id="sso-ldap-base-dn" type="text" bind:value={ssoForm.ldap_base_dn} />
        {/snippet}
      </FormField>
      <FormField id="sso-ldap-filter" label={t('admin.sso.field.ldap_user_filter')}>
        {#snippet children()}
          <input id="sso-ldap-filter" type="text" bind:value={ssoForm.ldap_user_filter} placeholder={'(uid={username})'} />
        {/snippet}
      </FormField>
    </div>
    <div class="toggle-row">
      <input id="sso-enabled" type="checkbox" bind:checked={ssoForm.enabled} />
      <label for="sso-enabled">{t('admin.sso.enable_provider')}</label>
    </div>
    <div class="inline-actions">
      <button class="btn-primary" type="button" onclick={saveSsoProvider} disabled={ssoSaving}>
        {ssoSaving ? t('common.saving') : editingSsoId ? t('admin.sso.update') : t('admin.sso.create')}
      </button>
      {#if editingSsoId}
        <button class="btn-secondary" type="button" onclick={resetSsoForm}>{t('common.cancel')}</button>
      {/if}
    </div>
  </div>
</div>

<style>
  h2 {
    margin: 0 0 12px;
  }
  h3 {
    margin: 18px 0 12px;
  }

  .section {
    padding: 16px;
    margin-bottom: 16px;
  }

  .toggle-row { display: flex; align-items: center; gap: 10px; font-size: 14px; cursor: pointer; }
  .toggle-row input[type='checkbox'] { width: 18px; height: 18px; }

  .inline-actions { display: flex; gap: 8px; margin-top: 16px; }

  .provider-list { display: flex; flex-direction: column; gap: 8px; margin-bottom: 16px; }

  .connection-result {
    margin-bottom: 12px;
    padding: 8px 10px;
    border: 1px solid var(--red);
    border-radius: var(--radius);
    color: var(--red);
    font-size: 13px;
  }
  .connection-result.success { border-color: var(--green-dim); color: var(--green); }

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
  .provider-meta .enabled { color: var(--green); }
  .provider-actions { display: flex; flex-wrap: wrap; gap: 6px; justify-content: flex-end; }

  .form-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 0 12px; }

  @media (max-width: 720px) {
    .provider-row { align-items: stretch; flex-direction: column; }
    .provider-actions { justify-content: flex-start; }
    .form-grid { grid-template-columns: 1fr; }
  }
</style>
