<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { page } from '$app/state';
  import { mirrors, type RepositoryMirror } from '$lib/api/client.svelte';
  import MirrorForm from '$lib/components/settings/MirrorForm.svelte';
  import MirrorStatusPanel from '$lib/components/settings/MirrorStatusPanel.svelte';
  import PushMirrorPanel from '$lib/components/settings/PushMirrorPanel.svelte';
  import { createT } from '$lib/i18n';
  import { createLatest } from '$lib/utils/latest';

  const t = createT();
  const latest = createLatest();
  const owner = $derived(page.params.owner!);
  const repo = $derived(page.params.repo!);

  let mirror = $state<RepositoryMirror | null>(null);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    void loadMirror();
  });

  async function loadMirror() {
    const reqOwner = owner;
    const reqRepo = repo;
    const isCurrent = latest.begin();
    try {
      loading = true;
      error = '';
      const data = await mirrors.get(reqOwner, reqRepo);
      if (!isCurrent()) return;
      mirror = data;
    } catch (err) {
      if (!isCurrent()) return;
      if ((err instanceof Error ? err.message : '').toLowerCase().includes('no mirror configured')) {
        mirror = null;
      } else {
        error = toErrorMessage(err, t('settings.mirror.load_failed'));
      }
    } finally {
      if (isCurrent()) {
        loading = false;
      }
    }
  }
</script>

<svelte:head>
  <title>{t('settings.mirror.title')} · {owner}/{repo} · IronForge</title>
</svelte:head>

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

    <PushMirrorPanel {owner} {repo} onChanged={loadMirror} />
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
