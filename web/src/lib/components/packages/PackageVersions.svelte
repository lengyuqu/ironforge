<script lang="ts">
  import { packages, type PackageVersionResponse } from '$lib/api/client.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    repo,
    format,
    name,
    packageName,
    versions,
    onRefresh,
  }: {
    owner: string;
    repo: string;
    format: string;
    /** URL 参数中的包名（scoped 名可能含编码斜杠） */
    name: string;
    /** 显示用包名 */
    packageName: string;
    versions: PackageVersionResponse[];
    onRefresh: () => void | Promise<void>;
  } = $props();

  let deletingVersion = $state<string | null>(null);
  let confirmDelete = $state<string | null>(null);
  let error = $state('');

  function getInstallCommand(ver: string): string {
    const f = format.toLowerCase();
    if (f === 'cargo') return `cargo add ${name}`;
    if (f === 'npm') return `npm install ${name}@${ver}`;
    if (f === 'pypi') return `pip install ${name}==${ver}`;
    if (f === 'maven') return `<version>${ver}</version>`;
    if (f === 'docker') return `docker pull ${owner}/${repo}:${ver}`;
    if (f === 'nuget') return `dotnet add package ${name} --version ${ver}`;
    if (f === 'rubygems') return `gem install ${name} --version ${ver}`;
    if (f === 'go')
      return `GOPROXY=<IronForge URL>/api/v1/repos/${owner}/${repo}/packages/go go get ${name}@${ver}`;
    if (f === 'helm') return `helm install my-release ${name} --version ${ver}`;
    if (f === 'composer') return `composer require ${name}:${ver}`;
    return `# install ${name} ${ver}`;
  }

  function copyInstall(ver: string) {
    navigator.clipboard.writeText(getInstallCommand(ver));
  }

  function formatSize(size?: number): string {
    if (!Number.isFinite(size)) return '';
    const units = ['B', 'KB', 'MB', 'GB'];
    let value = Number(size);
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
      value /= 1024;
      unit += 1;
    }
    return `${value >= 10 || unit === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`;
  }

  function packageDownloadUrl(ver: string, filename: string): string {
    return packages.downloadUrl(owner, repo, format, name, ver, filename);
  }

  async function handleDeleteVersion(version: string) {
    deletingVersion = version;
    error = '';
    try {
      await packages.delete(owner, repo, format, name, version);
      confirmDelete = null;
      await onRefresh();
    } catch (e: any) {
      error = toErrorMessage(e, t('common.delete') || 'Delete failed');
    } finally {
      deletingVersion = null;
    }
  }
</script>

{#if error}
  <div class="error-banner">{error}</div>
{/if}

<div class="versions-section">
  <h2>{t('packages.version') || 'Versions'}</h2>
  {#each versions as version (version.id)}
    <div class="version-card">
      <div class="version-header">
        <span class="version-name">v{version.version}</span>
        <div class="version-actions">
          <button class="copy-btn" onclick={() => copyInstall(version.version)}>
            {t('common.copy') || 'Copy'} {t('packages.install') || 'Install'}
          </button>
          <button
            class="danger-btn"
            onclick={() => {
              deletingVersion = version.version;
              confirmDelete = version.version;
            }}
          >
            {t('common.delete')}
          </button>
        </div>
      </div>

      {#if version.files && version.files.length > 0}
        <div class="version-files">
          {#each version.files as file (file.id)}
            <a class="file-link" href={packageDownloadUrl(version.version, file.filename)}>
              <span>{file.filename}</span>
              {#if file.size !== undefined}
                <span class="file-size">{formatSize(file.size)}</span>
              {/if}
            </a>
          {/each}
        </div>
      {/if}

      {#if confirmDelete === version.version}
        <div class="delete-confirm">
          <span>
            {t('packages.delete_confirm', { name: packageName, version: version.version }) ||
              `Delete ${packageName} ${version.version}?`}
          </span>
          <button class="danger-btn" onclick={() => handleDeleteVersion(version.version)}>
            {t('common.delete')}
          </button>
          <button
            class="secondary-btn"
            onclick={() => {
              confirmDelete = null;
              deletingVersion = null;
            }}
          >
            {t('common.cancel')}
          </button>
        </div>
      {/if}
    </div>
  {/each}
</div>

<style>
  h2 {
    font-size: 18px;
    font-weight: 600;
  }

  .versions-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
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
    font-weight: 600;
    font-size: 15px;
  }

  .version-actions {
    display: flex;
    gap: 8px;
  }

  .copy-btn {
    padding: 4px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 12px;
    cursor: pointer;
    color: var(--text-primary);
  }

  .copy-btn:hover {
    border-color: var(--accent);
    color: var(--accent);
  }

  .danger-btn {
    padding: 4px 12px;
    background: rgba(248, 81, 73, 0.1);
    border: 1px solid rgba(248, 81, 73, 0.4);
    border-radius: var(--radius);
    font-size: 12px;
    cursor: pointer;
    color: #f85149;
  }

  .danger-btn:hover {
    background: rgba(248, 81, 73, 0.2);
  }

  .secondary-btn {
    padding: 4px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 12px;
    cursor: pointer;
    color: var(--text-primary);
  }

  .version-files {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 12px;
  }

  .file-link {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 6px 10px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 13px;
    color: var(--accent);
    text-decoration: none;
  }

  .file-link:hover {
    text-decoration: underline;
  }

  .file-size {
    color: var(--text-muted);
  }

  .delete-confirm {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 12px;
    padding: 10px 12px;
    background: rgba(248, 81, 73, 0.08);
    border: 1px solid rgba(248, 81, 73, 0.35);
    border-radius: var(--radius);
    font-size: 13px;
    flex-wrap: wrap;
  }

  .error-banner {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 10px 12px;
    border-radius: var(--radius);
    margin-bottom: 16px;
  }
</style>
