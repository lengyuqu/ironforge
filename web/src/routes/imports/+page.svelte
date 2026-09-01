<script lang="ts">
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
    } catch (e: any) {
      error = e.message || t('errors.load_failed');
    } finally {
      loading = false;
    }
  }

  function handleDeleted(id: number) {
    taskList = taskList.filter((task) => task.id !== id);
  }
</script>

<svelte:head>
  <title>Imports · IronForge</title>
</svelte:head>

<div class="page-container">
  <div class="page-header">
    <div>
      <h1>Imports</h1>
      <p class="subtitle">Migrate repositories and project data from GitHub, GitLab, Gitea, or Git remotes.</p>
    </div>
    <button class="btn-secondary" type="button" onclick={loadImports} disabled={loading}>Refresh</button>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <section class="panel">
    <h2>New import</h2>
    <ImportForm initialOwner={initialOwner} onStarted={loadImports} />
  </section>

  <section class="panel">
    <h2>Import tasks</h2>
    {#if loading}
      <p class="muted">Loading imports...</p>
    {:else if taskList.length === 0}
      <p class="muted">No import tasks yet.</p>
    {:else}
      <ImportTaskTable tasks={taskList} onDeleted={handleDeleted} />
    {/if}
  </section>
</div>

<style>
  .page-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 16px;
    margin-bottom: 20px;
  }

  h1 {
    margin: 0 0 4px;
    font-size: 24px;
  }

  h2 {
    margin: 0 0 16px;
    font-size: 18px;
  }

  .subtitle,
  .muted {
    color: var(--text-secondary);
  }

  .panel {
    margin-bottom: 20px;
    padding: 16px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .btn-secondary {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    color: var(--text-primary);
    border-radius: 6px;
    padding: 8px 12px;
    cursor: pointer;
    font-weight: 600;
  }

  button:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .error-banner {
    margin-bottom: 16px;
    padding: 10px 12px;
    border-radius: 6px;
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
  }

  @media (max-width: 720px) {
    .page-header {
      display: block;
    }
  }
</style>
