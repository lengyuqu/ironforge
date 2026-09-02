<script lang="ts">
  // Package upload form — self-contained: format picker with support
  // notes, file input and optional metadata fields; publishes via the
  // packages API and reports the result to the parent via callbacks.
  // Inline error/success banners stay inside the form.
  import { packages } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import {
    PACKAGE_FORMATS,
    packageFormatOptionLabel,
    packageFormatSupportLabel,
    packageFormatUsesGenericFallback,
  } from '$lib/packageFormats';

  interface Props {
    owner: string;
    repo: string;
    /** Called after a successful publish. */
    onUploaded: () => void | Promise<void>;
  }

  let { owner, repo, onUploaded }: Props = $props();

  const t = createT();

  let format = $state('cargo');
  let packageFile = $state<File | null>(null);
  let packageName = $state('');
  let packageVersion = $state('');
  let description = $state('');
  let homepage = $state('');
  let repositoryUrl = $state('');
  let semver = $state('');

  let uploading = $state(false);
  let error = $state('');
  let success = $state('');

  function selectedFileLabel() {
    if (!packageFile) return t('packages.file');
    return packageFile.name;
  }

  function handleFileChange(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const files = input.files;
    packageFile = files && files.length > 0 ? files[0] : null;
    error = '';
  }

  async function handleUpload(event: Event) {
    event.preventDefault();
    if (!packageFile) {
      error = t('packages.file_required', 'Package file is required');
      return;
    }

    uploading = true;
    error = '';
    success = '';

    const metadata = {
      name: packageName.trim() || undefined,
      version: packageVersion.trim() || undefined,
      description: description.trim() || undefined,
      homepage: homepage.trim() || undefined,
      repository_url: repositoryUrl.trim() || undefined,
      semver: semver.trim() || undefined,
    };

    try {
      await packages.publish(owner, repo, format, packageFile, metadata);
      success = t('packages.upload_success');
      await onUploaded();
    } catch (e: unknown) {
      error = toErrorMessage(e, t('errors.save_failed', 'Upload failed'));
    } finally {
      uploading = false;
    }
  }
</script>

<form class="package-form" onsubmit={handleUpload}>
  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if success}
    <div class="success-banner">{success}</div>
  {/if}

  <div class="form-group">
    <label for="format">{t('packages.format')}</label>
    <select id="format" bind:value={format} class="select">
      {#each PACKAGE_FORMATS as f (f)}
        <option value={f}>{packageFormatOptionLabel(f)}</option>
      {/each}
    </select>
    <p class="format-note" class:fallback={packageFormatUsesGenericFallback(format)}>
      {packageFormatSupportLabel(format)}
    </p>
  </div>

  <div class="form-group">
    <label for="package-file">{t('packages.file')}</label>
    <div class="file-input-wrap">
      <input id="package-file" type="file" onchange={handleFileChange} />
      <span class="file-label">{selectedFileLabel()}</span>
    </div>
  </div>

  <div class="form-group">
    <label for="name">Name</label>
    <input id="name" type="text" bind:value={packageName} class="input" placeholder="Package name" />
  </div>

  <div class="form-group">
    <label for="version">{t('packages.version')}</label>
    <input
      id="version"
      type="text"
      bind:value={packageVersion}
      class="input"
      placeholder={t('packages.version')}
    />
  </div>

  <div class="form-group">
    <label for="description">{t('packages.description')}</label>
    <textarea id="description" bind:value={description} class="textarea" rows="3"></textarea>
  </div>

  <div class="form-group">
    <label for="homepage">Homepage</label>
    <input id="homepage" type="text" bind:value={homepage} class="input" />
  </div>

  <div class="form-group">
    <label for="repository-url">Repository URL</label>
    <input id="repository-url" type="text" bind:value={repositoryUrl} class="input" />
  </div>

  <div class="form-group">
    <label for="semver">Semver</label>
    <input id="semver" type="text" bind:value={semver} class="input" />
  </div>

  <div class="form-actions">
    <button type="submit" class="btn-primary" disabled={uploading || !packageFile}>
      {uploading ? t('packages.uploading', 'Uploading...') : t('packages.upload')}
    </button>
  </div>
</form>

<style>
  .package-form {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 24px;
  }

  .error-banner,
  .success-banner {
    padding: 10px 12px;
    border-radius: var(--radius);
    font-size: 13px;
    margin-bottom: 16px;
  }

  .error-banner {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    border: 1px solid rgba(248, 81, 73, 0.35);
  }

  .success-banner {
    color: #1a7f37;
    background: rgba(26, 127, 55, 0.1);
    border: 1px solid rgba(26, 127, 55, 0.35);
  }

  .form-group {
    margin-bottom: 18px;
  }

  .form-group label {
    display: block;
    margin-bottom: 6px;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .input,
  .select,
  .textarea {
    width: 100%;
    padding: 8px 12px;
    font-size: 14px;
    color: var(--text-primary);
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-sizing: border-box;
  }

  .textarea {
    resize: vertical;
    min-height: 80px;
    font-family: inherit;
  }

  .format-note {
    margin: 6px 0 0;
    font-size: 12px;
    color: var(--text-muted);
  }

  .format-note.fallback {
    color: var(--yellow);
  }

  .file-input-wrap {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .file-label {
    font-size: 13px;
    color: var(--text-secondary);
    overflow-wrap: anywhere;
  }

  .form-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 8px;
  }

  .btn-primary {
    padding: 8px 20px;
    background: var(--accent);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }

  .btn-primary:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
