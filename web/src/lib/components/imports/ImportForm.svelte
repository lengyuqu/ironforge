<script lang="ts">
  import { imports, type StartImportPayload } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    initialOwner = '',
    onStarted,
  }: {
    initialOwner?: string;
    onStarted: () => void | Promise<void>;
  } = $props();

  type ImportPlatform = 'github' | 'gitlab' | 'gitea' | 'git';

  let platform = $state<ImportPlatform>('github');
  let sourceUrl = $state('');
  let targetOwner = $state(initialOwner);
  let targetName = $state('');
  let authToken = $state('');
  let importRepo = $state(true);
  let importIssues = $state(true);
  let importPullRequests = $state(true);
  let importWiki = $state(false);
  let importReleases = $state(true);
  let importLabels = $state(true);
  let importMilestones = $state(true);
  let submitting = $state(false);

  function supportsMetadataImport(value = platform): boolean {
    return value === 'github' || value === 'gitlab';
  }

  function sourcePlaceholder(): string {
    switch (platform) {
      case 'gitlab':
        return 'https://gitlab.com/example/project';
      case 'gitea':
        return 'https://gitea.example.com/example/project.git';
      case 'git':
        return 'https://git.example.com/example/project.git';
      default:
        return 'https://github.com/example/project';
    }
  }

  $effect(() => {
    if (!supportsMetadataImport(platform)) {
      importIssues = false;
      importPullRequests = false;
      importWiki = false;
      importReleases = false;
      importLabels = false;
      importMilestones = false;
    }
  });

  async function startImport(e: Event) {
    e.preventDefault();

    const payload: StartImportPayload = {
      platform,
      source_url: sourceUrl.trim(),
      target_owner: targetOwner.trim(),
      import_repo: importRepo,
      import_issues: importIssues,
      import_pull_requests: importPullRequests,
      import_wiki: importWiki,
      import_releases: importReleases,
      import_labels: importLabels,
      import_milestones: importMilestones,
    };

    if (targetName.trim()) payload.target_name = targetName.trim();
    if (authToken.trim()) payload.auth_token = authToken.trim();

    submitting = true;
    try {
      await imports.start(payload);
      sourceUrl = '';
      targetName = '';
      authToken = '';
      toast.success(t('imports.queued'));
      await onStarted();
    } catch (e) {
      toast.error(toErrorMessage(e, t('errors.start_failed')));
    } finally {
      submitting = false;
    }
  }
</script>

<form class="import-form" onsubmit={startImport}>
  <label>
    {t('imports.platform')}
    <select bind:value={platform}>
      <option value="github">GitHub</option>
      <option value="gitlab">GitLab</option>
      <option value="gitea">Gitea</option>
      <option value="git">Git</option>
    </select>
  </label>

  <label class="wide">
    {t('imports.source_url')}
    <input type="url" bind:value={sourceUrl} placeholder={sourcePlaceholder()} required />
  </label>

  <label>
    {t('imports.target_owner')}
    <input type="text" bind:value={targetOwner} required />
  </label>

  <label>
    {t('imports.target_repo')}
    <input type="text" bind:value={targetName} placeholder={t('imports.target_repo_placeholder')} />
  </label>

  <label class="wide">
    {t('imports.source_token')}
    <input type="password" bind:value={authToken} autocomplete="off" placeholder={t('imports.token_placeholder')} />
  </label>

  <fieldset class="wide options">
    <legend>{t('imports.content')}</legend>
    <label><input type="checkbox" bind:checked={importRepo} /> {t('imports.content_repo')}</label>
    <label><input type="checkbox" bind:checked={importIssues} disabled={!supportsMetadataImport()} /> {t('imports.content_issues')}</label>
    <label><input type="checkbox" bind:checked={importPullRequests} disabled={!supportsMetadataImport()} /> {t('imports.content_pulls')}</label>
    <label><input type="checkbox" bind:checked={importWiki} disabled={!supportsMetadataImport()} /> {t('imports.content_wiki')}</label>
    <label><input type="checkbox" bind:checked={importReleases} disabled={!supportsMetadataImport()} /> {t('imports.content_releases')}</label>
    <label><input type="checkbox" bind:checked={importLabels} disabled={!supportsMetadataImport()} /> {t('imports.content_labels')}</label>
    <label><input type="checkbox" bind:checked={importMilestones} disabled={!supportsMetadataImport()} /> {t('imports.content_milestones')}</label>
  </fieldset>

  <div class="actions wide">
    <button class="btn-primary" type="submit" disabled={submitting || !sourceUrl.trim() || !targetOwner.trim()}>
      {submitting ? t('imports.starting') : t('imports.start')}
    </button>
  </div>
</form>

<style>
  .import-form {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 600;
  }

  input,
  select {
    min-width: 0;
  }

  .wide {
    grid-column: 1 / -1;
  }

  .options {
    display: flex;
    flex-wrap: wrap;
    gap: 10px 18px;
    margin: 0;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .options legend {
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 600;
    padding: 0 4px;
  }

  .options label {
    flex-direction: row;
    align-items: center;
    font-weight: 500;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
  }

  button:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  @media (max-width: 720px) {
    .import-form {
      display: block;
    }

    .import-form label,
    .options,
    .actions {
      margin-top: 12px;
    }
  }
</style>
