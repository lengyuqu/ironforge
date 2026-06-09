<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { packages } from '$lib/api/client';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let format = $derived($page.params.format!);
  let name = $derived($page.params.name!);

  let packageInfo = $state<any>(null);
  let versions = $state<string[]>([]);
  let loading = $state(true);
  let error = $state('');
  let deletingVersion = $state<string | null>(null);
  let confirmDelete = $state<string | null>(null);

  const formatLabels: Record<string, string> = {
    cargo: 'Cargo',
    npm: 'npm',
    pypi: 'PyPI',
    maven: 'Maven',
    docker: 'Docker',
    nuget: 'NuGet',
    rubygems: 'RubyGems',
    helm: 'Helm',
    generic: 'Generic',
  };

  $effect(() => {
    loadPackage();
  });

  async function loadPackage() {
    loading = true;
    error = '';
    try {
      const res = await packages.get(owner!, repo!, format!, name!);
      packageInfo = res;
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function loadVersions() {
    try {
      const res = await packages.getVersions(owner!, repo!, format!, name!);
      versions = res.versions || [];
    } catch (e: any) {
      error = e.message;
    }
  }

  async function handleDeleteVersion(version: string) {
    try {
      await packages.delete(owner!, repo!, format!, name!, version);
      confirmDelete = null;
      await loadVersions();
    } catch (e: any) {
      error = e.message;
    }
  }

  function getInstallCommand(ver: string): string {
    const f = format!.toLowerCase();
    if (f === 'cargo') return `cargo add ${name!}`;
    if (f === 'npm') return `npm install ${name!}@${ver}`;
    if (f === 'pypi') return `pip install ${name!}==${ver}`;
    if (f === 'maven') return `<version>${ver}</version>`;
    if (f === 'docker') return `docker pull ${owner}/${repo}:${ver}`;
    if (f === 'nuget') return `dotnet add package ${name!} --version ${ver}`;
    if (f === 'rubygems') return `gem install ${name!} --version ${ver}`;
    if (f === 'helm') return `helm install my-release ${name!} --version ${ver}`;
    return `# install ${name!} ${ver}`;
  }

  function copyInstall(ver: string) {
    navigator.clipboard.writeText(getInstallCommand(ver));
  }
</script>

<svelte:head>
  <title>{name} · {formatLabels[format!] || format} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="repo-page">
  <RepoHeader owner={owner!} repo={repo!} activeTab="packages" />

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="loading-text">{t('common.loading')}</p>
  {:else if !packageInfo}
    <div class="empty">
      <p>{t('packages.no_packages')}</p>
    </div>
  {:else}
    <div class="package-detail">
      <div class="package-header">
        <h1>{packageInfo.name}</h1>
        {#if packageInfo.latest_version}
          <span class="version-badge">v{packageInfo.latest_version}</span>
        {/if}
      </div>

      {#if packageInfo.description}
        <p class="package-desc">{packageInfo.description}</p>
      {/if}

      <div class="package-meta">
        {#if packageInfo.created_at}
          <span>{t('common.created', { date: formatDate(packageInfo.created_at) })}</span>
        {/if}
      </div>

      <!-- Version list -->
      <div class="versions-section">
        <h2>{t('packages.version') || 'Versions'}</h2>
        {#each versions as ver}
          <div class="version-card">
            <div class="version-header">
              <span class="version-name">v{ver}</span>
              <div class="version-actions">
                <button class="copy-btn" onclick={() => copyInstall(ver)}>
                  {t('common.copy') || 'Copy'} {t('packages.install') || 'Install'}
                </button>
                <button class="danger-btn" onclick={() => { deletingVersion = ver; confirmDelete = ver; }}>
                  {t('common.delete')}
                </button>
              </div>
            </div>

            {#if confirmDelete === ver}
              <div class="delete-confirm">
                <span>{t('packages.delete_confirm', { name: packageInfo.name, version: ver }) || `Delete ${packageInfo.name} ${ver}?`}</span>
                <button class="danger-btn" onclick={() => handleDeleteVersion(ver)}>
                  {t('common.delete')}
                </button>
                <button class="secondary-btn" onclick={() => { confirmDelete = null; deletingVersion = null; }}>
                  {t('common.cancel')}
                </button>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .repo-page {
    max-width: 900px;
    margin: 0 auto;
    padding: 24px;
  }

  .error-banner {
    background: rgba(248, 81, 73, 0.1);
    border: 1px solid var(--red-dim);
    color: var(--red);
    border-radius: var(--radius);
    padding: 10px 14px;
    font-size: 13px;
    margin-bottom: 16px;
  }

  .loading-text {
    color: var(--text-secondary);
    text-align: center;
    padding: 48px;
  }

  .empty {
    text-align: center;
    padding: 48px;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .package-detail {
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .package-header {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }

  h1 {
    font-size: 24px;
    font-weight: 600;
  }

  .version-badge {
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 12px;
    font-weight: 600;
    background: var(--green-dim);
    color: #fff;
  }

  .package-desc {
    font-size: 14px;
    color: var(--text-secondary);
    line-height: 1.6;
  }

  .package-meta {
    font-size: 13px;
    color: var(--text-muted);
  }

  .versions-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  h2 {
    font-size: 18px;
    font-weight: 600;
  }

  .version-card {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px;
  }

  .version-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }

  .version-name {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .version-actions {
    display: flex;
    gap: 8px;
  }

  .copy-btn {
    padding: 4px 10px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 12px;
    cursor: pointer;
    color: var(--text-primary);
  }
  .copy-btn:hover { background: var(--bg-hover); }

  .danger-btn {
    padding: 4px 10px;
    background: var(--red-dim);
    border: 1px solid var(--red);
    border-radius: var(--radius);
    font-size: 12px;
    cursor: pointer;
    color: #fff;
  }
  .danger-btn:hover { background: var(--red); }

  .secondary-btn {
    padding: 4px 10px;
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 12px;
    cursor: pointer;
    color: var(--text-primary);
  }
  .secondary-btn:hover { background: var(--bg-hover); }

  .delete-confirm {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    margin-top: 12px;
    padding: 8px 12px;
    background: rgba(248, 81, 73, 0.05);
    border: 1px solid var(--red-dim);
    border-radius: var(--radius);
  }
  .delete-confirm span { color: var(--text-secondary); }

  @media (max-width: 600px) {
    .version-header {
      flex-direction: column;
      align-items: flex-start;
    }
    .version-actions {
      flex-direction: column;
      width: 100%;
    }
  }
</style>
