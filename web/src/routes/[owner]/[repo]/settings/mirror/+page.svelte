<script lang="ts">
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
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
    <ErrorBanner message={error} />
  {/if}

  {#if loading}
    <LoadingState />
  {:else}
    <MirrorForm {owner} {repo} {mirror} onChanged={loadMirror} />

    {#if mirror}
      <MirrorStatusPanel {mirror} />
    {:else}
      <EmptyState title={t('settings.mirror.empty')} />
    {/if}
  {/if}
</div>

<style>
  .mirror-page {
    max-width: 900px;
  }

  h1 {
    margin: 0 0 0.5rem;
    color: var(--text-primary);
  }

  p {
    margin: 0;
    color: var(--text-secondary);
    font-size: 0.95rem;
  }

</style>
