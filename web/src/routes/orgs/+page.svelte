<script lang="ts">
  import { goto } from '$app/navigation';
  import { isAuthReady, isLoggedIn } from '$lib/stores/auth.svelte';
  import { orgs } from '$lib/api/client.svelte';
  import type { Organization, OrgSummary } from '$lib/types/entities';
  import CreateOrgForm from '$lib/components/orgs/CreateOrgForm.svelte';
  import OrgList from '$lib/components/orgs/OrgList.svelte';
  import { createT } from '$lib/i18n';
  import { onMount } from 'svelte';

  const t = createT();

  let organizations = $state<Organization[]>([]);
  let error = $state('');
  let loading = $state(true);
  let showCreate = $state(false);

  // F-003: Auth guard
  $effect(() => {
    if (!isAuthReady()) return;
    if (!isLoggedIn()) {
      goto('/login');
    }
  });

  async function loadOrgs() {
    loading = true;
    error = '';
    try {
      organizations = await orgs.list();
    } catch (e: any) {
      error = e.message || t('errors.load_failed');
    } finally {
      loading = false;
    }
  }

  function toggleCreate() {
    showCreate = !showCreate;
  }

  async function handleCreated(org: OrgSummary) {
    showCreate = false;
    await loadOrgs();
    goto(`/orgs/${org.name}`);
  }

  onMount(loadOrgs);
</script>

<div class="container">
  <div class="page-header">
    <div>
      <h1>{t('orgs.title')}</h1>
      <p>{t('orgs.subtitle')}</p>
    </div>
    <button
      type="button"
      class="btn-primary header-action"
      onclick={toggleCreate}
      aria-expanded={showCreate}
    >
      {showCreate ? t('common.cancel') : t('orgs.create_action')}
    </button>
  </div>

  {#if error}
    <div class="error" role="alert">{error}</div>
  {/if}

  {#if showCreate}
    <CreateOrgForm onCreated={handleCreated} />
  {/if}

  <OrgList {organizations} {loading} onCreate={() => (showCreate = true)} />
</div>

<style>
  .page-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 1rem;
    margin-bottom: 1.5rem;
  }

  h1 {
    color: var(--text-primary);
    margin: 0 0 0.4rem;
  }

  .page-header p {
    color: var(--text-secondary);
    margin: 0;
  }

  .header-action {
    white-space: nowrap;
  }

  .btn-primary {
    border: none;
    border-radius: 6px;
    padding: 0.6rem 1.2rem;
    font-size: 0.95rem;
    cursor: pointer;
    background: var(--accent);
    color: white;
  }

  .error {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    border: 1px solid rgba(248, 81, 73, 0.35);
    padding: 0.65rem 0.85rem;
    border-radius: 6px;
    margin-bottom: 1rem;
  }

  @media (max-width: 700px) {
    .page-header {
      flex-direction: column;
    }

    .header-action,
    .btn-primary {
      width: 100%;
    }
  }
</style>
