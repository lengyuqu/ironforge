<script lang="ts">
  // SSO providers section — orchestrates the provider list and the
  // create/edit form: loading, saving, enable/disable, delete, test connection.
  import SsoProviderList from '$lib/components/admin/SsoProviderList.svelte';
  import SsoProviderForm from '$lib/components/admin/SsoProviderForm.svelte';
  import { createT } from '$lib/i18n';
  import { confirmDialog } from '$lib/components/confirm.svelte';
  import { admin, type AdminSsoProvider, type SsoProviderPayload } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import {
    emptyLdapGroupSyncForm,
    emptySsoProviderForm,
    formFromProvider,
    isLdapGroupSyncConfigured,
    ldapGroupSyncFormFromProvider,
    ldapGroupSyncPayloadFromForm,
    ssoPayloadFromForm,
    togglePayload,
    validateLdapGroupSyncForm,
    type LdapGroupSyncForm,
  } from '$lib/components/admin/ssoForm';

  const t = createT();

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
  let groupSyncForm = $state<LdapGroupSyncForm>(emptyLdapGroupSyncForm());

  // Snapshot the initial list (component mounts once per page load).
  providers = initialProviders;

  async function reloadProviders() {
    providers = await admin.listSsoProviders();
  }

  function editSsoProvider(provider: AdminSsoProvider) {
    editingSsoId = provider.id;
    ssoForm = formFromProvider(provider);
    groupSyncForm = ldapGroupSyncFormFromProvider(provider);
  }

  function resetSsoForm() {
    editingSsoId = null;
    ssoForm = emptySsoProviderForm();
    groupSyncForm = emptyLdapGroupSyncForm();
  }

  async function saveSsoProvider() {
    if (!ssoForm.name.trim() || !ssoForm.slug.trim()) {
      toast.error(t('admin.sso.name_slug_required'));
      return;
    }

    // Validated before the provider write so a half-filled group block cannot
    // leave the provider saved and the group sync silently unconfigured.
    if (ssoForm.provider_type === 'ldap') {
      const groupError = validateLdapGroupSyncForm(groupSyncForm);
      if (groupError) {
        const key =
          groupError === 'base_dn'
            ? 'admin.sso.group_sync.group_sync_err_base_dn'
            : groupError === 'filter'
              ? 'admin.sso.group_sync.group_sync_err_filter'
              : 'admin.sso.group_sync.group_sync_err_org';
        toast.error(t(key));
        return;
      }
    }

    try {
      ssoSaving = true;
      const payload = ssoPayloadFromForm(ssoForm);
      let savedId = editingSsoId;
      if (editingSsoId) {
        await admin.updateSsoProvider(editingSsoId, payload);
        toast.success(t('admin.sso.updated'));
      } else {
        // The group endpoint needs an id, so a new provider is created first.
        const created = await admin.createSsoProvider(payload);
        savedId = created.id;
        toast.success(t('admin.sso.created'));
      }

      if (savedId && ssoForm.provider_type === 'ldap') {
        // Editing always writes (all-empty switches the sync off); a brand new
        // provider only writes when something was filled in.
        const shouldWrite = editingSsoId !== null || isLdapGroupSyncConfigured(groupSyncForm);
        if (shouldWrite) {
          await admin.setLdapGroupSync(savedId, ldapGroupSyncPayloadFromForm(groupSyncForm));
        }
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
      await admin.updateSsoProvider(provider.id, togglePayload(provider));
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

  <SsoProviderList
    {providers}
    testingId={testingSsoId}
    testResult={ssoTestResult}
    onTest={testSsoProvider}
    onToggle={toggleSsoProvider}
    onEdit={editSsoProvider}
    onDelete={deleteSsoProvider}
  />

  <SsoProviderForm
    bind:form={ssoForm}
    bind:groupSyncForm
    editing={editingSsoId !== null}
    saving={ssoSaving}
    onSave={saveSsoProvider}
    onCancel={resetSsoForm}
  />
</div>

<style>
  h2 {
    margin: 0 0 12px;
  }

  .section {
    padding: 16px;
    margin-bottom: 16px;
  }
</style>
