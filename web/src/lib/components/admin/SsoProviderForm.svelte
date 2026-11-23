<script lang="ts">
  import FormField from '$lib/components/FormField.svelte';
  import { createT } from '$lib/i18n';
  import type { SsoProviderPayload } from '$lib/api/client.svelte';
  import type { LdapGroupSyncForm } from '$lib/components/admin/ssoForm';

  interface Props {
    form: SsoProviderPayload;
    /** Group-sync block state; only rendered for a saved LDAP provider. */
    groupSyncForm: LdapGroupSyncForm;
    /** True while editing an existing provider (blank secrets are kept). */
    editing: boolean;
    saving: boolean;
    onSave: () => void;
    onCancel: () => void;
  }

  let {
    form = $bindable(),
    groupSyncForm = $bindable(),
    editing,
    saving,
    onSave,
    onCancel,
  }: Props = $props();

  const t = createT();
</script>

<div class="sso-form">
  <h3>{editing ? t('admin.sso.edit_title') : t('admin.sso.add_title')}</h3>
  <div class="form-grid">
    <FormField id="sso-name" label={t('admin.sso.field.name')}>
      {#snippet children()}
        <input id="sso-name" type="text" bind:value={form.name} placeholder={t('admin.sso.name_placeholder')} />
      {/snippet}
    </FormField>
    <FormField id="sso-slug" label={t('admin.sso.field.slug')}>
      {#snippet children()}
        <input id="sso-slug" type="text" bind:value={form.slug} placeholder="google" />
      {/snippet}
    </FormField>
    <FormField id="sso-type" label={t('admin.sso.field.type')}>
      {#snippet children()}
        <select id="sso-type" bind:value={form.provider_type}>
          <option value="oauth2">{t('admin.sso.type_oauth2')}</option>
          <option value="ldap">LDAP</option>
        </select>
      {/snippet}
    </FormField>
    <FormField id="sso-client-id" label={t('admin.sso.field.client_id')}>
      {#snippet children()}
        <input id="sso-client-id" type="text" bind:value={form.client_id} />
      {/snippet}
    </FormField>
    <FormField id="sso-client-secret" label={t('admin.sso.field.client_secret')}>
      {#snippet children()}
        <input
          id="sso-client-secret"
          type="password"
          bind:value={form.client_secret}
          placeholder={editing ? t('admin.sso.keep_secret') : ''}
        />
      {/snippet}
    </FormField>
    <FormField id="sso-discovery-url" label={t('admin.sso.field.discovery_url')}>
      {#snippet children()}
        <input id="sso-discovery-url" type="url" bind:value={form.discovery_url} />
      {/snippet}
    </FormField>
    <FormField id="sso-scopes" label={t('admin.sso.field.scopes')}>
      {#snippet children()}
        <input id="sso-scopes" type="text" bind:value={form.scopes} />
      {/snippet}
    </FormField>
    <FormField id="sso-icon-url" label={t('admin.sso.field.icon_url')}>
      {#snippet children()}
        <input id="sso-icon-url" type="url" bind:value={form.icon_url} />
      {/snippet}
    </FormField>
    <FormField id="sso-ldap-host" label={t('admin.sso.field.ldap_host')}>
      {#snippet children()}
        <input id="sso-ldap-host" type="text" bind:value={form.ldap_host} placeholder={t('admin.sso.ldap_host_placeholder')} />
      {/snippet}
    </FormField>
    <FormField id="sso-ldap-port" label={t('admin.sso.field.ldap_port')}>
      {#snippet children()}
        <input id="sso-ldap-port" type="number" min="1" bind:value={form.ldap_port} />
      {/snippet}
    </FormField>
    <FormField id="sso-ldap-bind-dn" label={t('admin.sso.field.ldap_bind_dn')}>
      {#snippet children()}
        <input id="sso-ldap-bind-dn" type="text" bind:value={form.ldap_bind_dn} />
      {/snippet}
    </FormField>
    <FormField id="sso-ldap-bind-password" label={t('admin.sso.field.ldap_bind_password')}>
      {#snippet children()}
        <input
          id="sso-ldap-bind-password"
          type="password"
          bind:value={form.ldap_bind_password}
          placeholder={editing ? t('admin.sso.keep_password') : ''}
        />
      {/snippet}
    </FormField>
    <FormField id="sso-ldap-base-dn" label={t('admin.sso.field.ldap_base_dn')}>
      {#snippet children()}
        <input id="sso-ldap-base-dn" type="text" bind:value={form.ldap_base_dn} />
      {/snippet}
    </FormField>
    <FormField id="sso-ldap-filter" label={t('admin.sso.field.ldap_user_filter')}>
      {#snippet children()}
        <input id="sso-ldap-filter" type="text" bind:value={form.ldap_user_filter} placeholder={'(uid={username})'} />
      {/snippet}
    </FormField>
  </div>

  {#if form.provider_type === 'ldap'}
    <div class="group-sync">
      <h4>{t('admin.sso.group_sync.group_sync_title')}</h4>
      <p class="hint">{t('admin.sso.group_sync.group_sync_hint')}</p>
      {#if editing}
        <div class="form-grid">
          <FormField id="sso-group-base-dn" label={t('admin.sso.group_sync.group_sync_base_dn')}>
            {#snippet children()}
              <input
                id="sso-group-base-dn"
                type="text"
                bind:value={groupSyncForm.base_dn}
                placeholder="ou=groups,dc=example,dc=com"
              />
            {/snippet}
          </FormField>
          <FormField id="sso-group-filter" label={t('admin.sso.group_sync.group_sync_filter')}>
            {#snippet children()}
              <input
                id="sso-group-filter"
                type="text"
                bind:value={groupSyncForm.filter}
                placeholder="(objectClass=groupOfNames)"
              />
            {/snippet}
          </FormField>
          <FormField id="sso-group-org" label={t('admin.sso.group_sync.group_sync_org')}>
            {#snippet children()}
              <input id="sso-group-org" type="text" inputmode="numeric" bind:value={groupSyncForm.team_org_id} />
            {/snippet}
          </FormField>
        </div>
      {:else}
        <!-- The endpoint needs a provider id, so a new provider is saved first. -->
        <p class="hint">{t('admin.sso.group_sync.group_sync_save_first')}</p>
      {/if}
    </div>
  {/if}
  <div class="toggle-row">
    <input id="sso-enabled" type="checkbox" bind:checked={form.enabled} />
    <label for="sso-enabled">{t('admin.sso.enable_provider')}</label>
  </div>
  <div class="inline-actions">
    <button class="btn-primary" type="button" onclick={onSave} disabled={saving}>
      {saving ? t('common.saving') : editing ? t('admin.sso.update') : t('admin.sso.create')}
    </button>
    {#if editing}
      <button class="btn-secondary" type="button" onclick={onCancel}>{t('common.cancel')}</button>
    {/if}
  </div>
</div>

<style>
  h3 {
    margin: 18px 0 12px;
  }

  .toggle-row { display: flex; align-items: center; gap: 10px; font-size: 14px; cursor: pointer; }
  .toggle-row input[type='checkbox'] { width: 18px; height: 18px; }

  .inline-actions { display: flex; gap: 8px; margin-top: 16px; }

  .group-sync { margin-top: 18px; padding-top: 14px; border-top: 1px solid var(--border-subtle, #2a2a2a); }
  .group-sync h4 { margin: 0 0 6px; font-size: 14px; }
  .group-sync .hint { margin: 0 0 10px; font-size: 12px; opacity: 0.75; }

  .form-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 0 12px; }

  @media (max-width: 720px) {
    .form-grid { grid-template-columns: 1fr; }
  }
</style>
