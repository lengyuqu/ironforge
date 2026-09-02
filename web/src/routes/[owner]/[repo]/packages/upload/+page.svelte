<script lang="ts">
  // Package upload page — orchestration layer: auth guard and routing.
  // The whole upload flow (format/file/metadata/publish) lives inside
  // PackageUploadForm.
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { isAuthReady, isLoggedIn } from '$lib/stores/auth.svelte';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import PackageUploadForm from '$lib/components/packages/PackageUploadForm.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  // F-003: Auth guard
  $effect(() => {
    if (!isAuthReady()) return;
    if (!isLoggedIn()) {
      goto('/login');
    }
  });

  function handleUploaded() {
    goto(`/${owner}/${repo}/packages`);
  }
</script>

<svelte:head>
  <title>{t('packages.upload')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="packages" />

  <div class="page-header">
    <h1>{t('packages.upload')}</h1>
    <a href={`/${owner}/${repo}/packages`} class="btn-secondary">
      {t('common.back', 'Back')}
    </a>
  </div>

  <PackageUploadForm {owner} {repo} onUploaded={handleUploaded} />
</div>

<style>
  .page-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 24px;
  }

  h1 {
    font-size: 24px;
    font-weight: 600;
  }

  .btn-secondary {
    padding: 8px 20px;
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-primary);
    font-size: 14px;
    font-weight: 500;
    text-decoration: none;
    cursor: pointer;
  }

  .btn-secondary:hover {
    background: var(--bg-hover);
  }
</style>
