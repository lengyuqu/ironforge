<script lang="ts">
  import { page } from '$app/stores';
  import { createT } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
</script>

<svelte:head>
  <title>{t('admin.runners.title')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<section class="panel surface">
  <div>
    <h1>{t('admin.runners.title')}</h1>
    <p>{t('admin.runners.repo_handoff')}</p>
  </div>
  <a class="btn-primary" href="/admin/runners">{t('admin.runners.open_admin')}</a>
</section>

<style>
  .panel {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 1rem;
    padding: 1rem;
  }

  h1 {
    margin: 0 0 0.5rem;
  }

  p {
    margin: 0;
    color: var(--text-secondary);
    line-height: 1.5;
  }

  @media (max-width: 600px) {
    .panel {
      flex-direction: column;
    }
  }
</style>
