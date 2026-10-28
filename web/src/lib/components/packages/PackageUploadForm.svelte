<script lang="ts">
  // Package upload form — self-contained: format picker with support
  // notes, file input and optional metadata fields; publishes via the
  // packages API and reports the result to the parent via callbacks.
  // Inline error/success banners stay inside the form.
  import FormField from '$lib/components/FormField.svelte';
  import SuccessBanner from '$lib/components/SuccessBanner.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
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

<form class="package-form surface" onsubmit={handleUpload}>
  {#if error}
    <ErrorBanner message={error} />
  {/if}

  {#if success}
    <SuccessBanner message={success} />
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

  <FormField id="name" label={t('packages.name')}>
    {#snippet children()}
      <input id="name" type="text" bind:value={packageName} class="input" placeholder={t('packages.name_placeholder')} />
    {/snippet}
  </FormField>

  <FormField id="version" label={t('packages.version')}>
    {#snippet children()}
      <input
        id="version"
        type="text"
        bind:value={packageVersion}
        class="input"
        placeholder={t('packages.version')}
      />
    {/snippet}
  </FormField>

  <FormField id="description" label={t('packages.description')}>
    {#snippet children()}
      <textarea id="description" bind:value={description} class="textarea" rows="3"></textarea>
    {/snippet}
  </FormField>

  <FormField id="homepage" label={t('packages.homepage')}>
    {#snippet children()}
      <input id="homepage" type="text" bind:value={homepage} class="input" />
    {/snippet}
  </FormField>

  <FormField id="repository-url" label={t('packages.repository_url')}>
    {#snippet children()}
      <input id="repository-url" type="text" bind:value={repositoryUrl} class="input" />
    {/snippet}
  </FormField>

  <FormField id="semver" label={t('packages.semver')}>
    {#snippet children()}
      <input id="semver" type="text" bind:value={semver} class="input" />
    {/snippet}
  </FormField>

  <div class="form-actions">
    <button type="submit" class="btn-primary" disabled={uploading || !packageFile}>
      {uploading ? t('packages.uploading', 'Uploading...') : t('packages.upload')}
    </button>
  </div>
</form>

<style>
  .package-form {
    padding: 24px;
  }

  .input,
  .select,
  .textarea {
    width: 100%;
    box-sizing: border-box;
  }

  .textarea {
    resize: vertical;
    min-height: 80px;
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
    justify-content: flex-end;
  }
</style>
