<script lang="ts">
  import { page } from '$app/stores';
  import { mirrors, type RepositoryMirror } from '$lib/api/client.svelte';
  import MirrorForm from '$lib/components/settings/MirrorForm.svelte';
  import MirrorStatusPanel from '$lib/components/settings/MirrorStatusPanel.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();
  const owner = $derived($page.params.owner!);
  const repo = $derived($page.params.repo!);

  let mirror = $state<RepositoryMirror | null>(null);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    loadMirror();
  });

  async function loadMirror() {
    try {
      loading = true;
      error = '';
      mirror = await mirrors.get(owner, repo);
    } catch (err: any) {
      if (String(err?.message || '').toLowerCase().includes('no mirror configured')) {
        mirror = null;
      } else {
        error = err.message || t('settings.mirror.load_failed');
      }
    } finally {
      loading = false;
    }
  }
</script>

<div class="mirror-page">
  <div class="page-header">
    <div>
      <h1>{t('settings.mirror.title')}</h1>
      <p>{t('settings.mirror.desc')}</p>
    </div>
  </div>

  {#if error}
    <div class="error-box">{error}</div>
  {/if}

  {#if loading}
    <div class="loading">{t('common.loading')}</div>
  {:else}
    <MirrorForm {owner} {repo} {mirror} onChanged={loadMirror} />

    {#if mirror}
      <MirrorStatusPanel {mirror} />
    {:else}
      <div class="empty-state">{t('settings.mirror.empty')}</div>
    {/if}
  {/if}
</div>

<style>
  .mirror-page {
    max-width: 900px;
  }

  .page-header {
    margin-bottom: 2rem;
  }

  h1 {
    font-size: 1.75rem;
    margin: 0 0 0.5rem;
    color: var(--text-primary);
  }

  p {
    margin: 0;
    color: var(--text-secondary);
    font-size: 0.95rem;
  }

  .error-box {
    padding: 1rem;
    border-radius: 6px;
    margin-bottom: 1rem;
    background: rgba(248, 81, 73, 0.12);
    color: var(--red);
  }

  .empty-state,
  .loading {
    padding: 1rem;
    border-radius: 6px;
    margin-bottom: 1rem;
    background: var(--bg-secondary);
    color: var(--text-secondary);
    border: 1px solid var(--border);
  }
</style>
