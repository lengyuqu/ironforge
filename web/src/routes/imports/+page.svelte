<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { goto } from '$app/navigation';
  import { imports, type ImportTask } from '$lib/api/client.svelte';
  import { isLoggedIn, getUser } from '$lib/stores/auth.svelte';
  import { createT } from '$lib/i18n';
  import ImportForm from '$lib/components/imports/ImportForm.svelte';
  import ImportTaskTable from '$lib/components/imports/ImportTaskTable.svelte';

  const t = createT();

  let taskList = $state<ImportTask[]>([]);
  let loading = $state(true);
  let error = $state('');

  let initialOwner = $state('');

  $effect(() => {
    if (!isLoggedIn()) {
      goto('/login');
      return;
    }

    if (!initialOwner) {
      initialOwner = getUser()?.username || '';
    }

    loadImports();
  });

  async function loadImports() {
    loading = true;
    error = '';
    try {
      taskList = await imports.list();
    } catch (e) {
      error = toErrorMessage(e, t('errors.load_failed'));
    } finally {
      loading = false;
    }
  }

  function handleDeleted(id: number) {
    taskList = taskList.filter((task) => task.id !== id);
  }
</script>

<svelte:head>
  <title>{t('imports.page_title')}</title>
</svelte:head>

<div class="page-container">
  <div class="page-header">
    <div>
      <h1>{t('nav.imports')}</h1>
      <p class="subtitle">{t('imports.subtitle')}</p>
    </div>
    <button class="btn-secondary" type="button" onclick={loadImports} disabled={loading}>{t('imports.refresh')}</button>
  </div>

  {#if error}
    <ErrorBanner message={error} />
  {/if}

  <section class="panel surface">
    <h2>{t('imports.new_title')}</h2>
    <ImportForm initialOwner={initialOwner} onStarted={loadImports} />
  </section>

  <section class="panel surface">
    <h2>{t('imports.tasks_title')}</h2>
    {#if loading}
      <LoadingState />
    {:else if taskList.length === 0}
      <EmptyState title={t('imports.empty')} />
    {:else}
      <ImportTaskTable tasks={taskList} onDeleted={handleDeleted} />
    {/if}
  </section>
</div>

<style>

  h1 {
    margin: 0 0 4px;
  }

  h2 {
    margin: 0 0 16px;
  }

  .panel {
    margin-bottom: 20px;
    padding: 16px;
  }

  button:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  @media (max-width: 720px) {
    .page-header {
      display: block;
    }
  }
</style>
