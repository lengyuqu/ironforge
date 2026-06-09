<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { packages } from '$lib/api/client';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let format = $derived($page.params.format!);

  let packageList = $state<any[]>([]);
  let loading = $state(true);
  let error = $state('');
  let currentPage = $state(1);
  let totalPages = $state(1);

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
    loadPackages();
  });

  async function loadPackages() {
    loading = true;
    error = '';
    try {
      const res = await packages.getFormat(owner!, repo!, format!);
      packageList = res.packages || [];
      totalPages = 1;
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function getInstallCommand(pkg: any): string {
    const f = format!.toLowerCase();
    const name = pkg.name;
    const version = pkg.latest_version || '';

    if (f === 'cargo') return `cargo add ${name}`;
    if (f === 'npm') return `npm install ${name}`;
    if (f === 'pypi') return `pip install ${name}`;
    if (f === 'maven') return `<dependency>\n  <groupId>...</groupId>\n  <artifactId>${name}</artifactId>\n  <version>${version}</version>\n</dependency>`;
    if (f === 'docker') return `docker pull ${owner}/${repo}:${version}`;
    if (f === 'nuget') return `dotnet add package ${name}`;
    if (f === 'rubygems') return `gem install ${name}`;
    if (f === 'helm') return `helm install my-release ${name}`;
    return `# install ${name}`;
  }
</script>

<svelte:head>
  <title>{formatLabels[format!] || format} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="repo-page">
  <RepoHeader owner={owner!} repo={repo!} activeTab="packages" />

  <div class="page-header">
    <h1>{t('packages.title')} — {formatLabels[format!] || format}</h1>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="loading-text">{t('common.loading')}</p>
  {:else if packageList.length === 0}
    <div class="empty">
      <p>{t('packages.no_packages')}</p>
    </div>
  {:else}
    <div class="package-list">
      {#each packageList as pkg}
        <div class="package-card">
          <div class="package-header">
            <a href="/{owner}/{repo}/packages/{format}/{pkg.name}" class="package-name">{pkg.name}</a>
            {#if pkg.latest_version}
              <span class="version-badge">v{pkg.latest_version}</span>
            {/if}
          </div>

          {#if pkg.description}
            <p class="package-desc">{pkg.description}</p>
          {/if}

          <div class="package-meta">
            {#if pkg.created_at}
              <span class="date">{t('common.created', { date: formatDate(pkg.created_at) })}</span>
            {/if}
          </div>

          <div class="install-section">
            <pre><code>{getInstallCommand(pkg)}</code></pre>
            <button class="copy-btn" onclick={() => navigator.clipboard.writeText(getInstallCommand(pkg))}>
              {t('common.copy') || 'Copy'}
            </button>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .repo-page {
    max-width: 900px;
    margin: 0 auto;
    padding: 24px;
  }

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

  .package-list {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .package-card {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 20px;
  }

  .package-header {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 8px;
  }

  .package-name {
    font-size: 18px;
    font-weight: 600;
    color: var(--accent);
    text-decoration: none;
  }
  .package-name:hover { text-decoration: underline; }

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
    margin-bottom: 8px;
  }

  .package-meta {
    font-size: 13px;
    color: var(--text-muted);
    margin-bottom: 12px;
  }

  .install-section {
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .install-section pre {
    flex: 1;
    margin: 0;
    overflow-x: auto;
  }

  .install-section code {
    font-size: 13px;
    color: var(--text-primary);
  }

  .copy-btn {
    padding: 4px 10px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 12px;
    cursor: pointer;
    color: var(--text-primary);
  }
  .copy-btn:hover { background: var(--bg-hover); }
</style>
