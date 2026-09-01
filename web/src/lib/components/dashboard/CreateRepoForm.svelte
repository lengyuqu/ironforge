<script lang="ts">
  import { repos } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    onCreated,
    onCancel,
  }: {
    owner: string;
    onCreated: (repoName: string) => void | Promise<void>;
    onCancel: () => void;
  } = $props();

  let newName = $state('');
  let newDesc = $state('');
  let newPrivate = $state(false);
  let autoInit = $state(true);
  let defaultBranch = $state('main');
  let selectedGitignore = $state('');
  let selectedLicense = $state('');
  let selectedReadme = $state('default');
  let selectedLabels = $state('default');
  let submitting = $state(false);

  // Q6.3: Client-side form validation
  const MAX_REPO_NAME_LEN = 100;
  const MAX_DESC_LEN = 255;
  // Repo names: alphanumeric, dash, underscore, dot; cannot start/end with dash/dot
  const REPO_NAME_RE = /^[a-zA-Z0-9][a-zA-Z0-9._-]*[a-zA-Z0-9]$|^[a-zA-Z0-9]$/;
  let nameError = $derived(
    newName.trim().length === 0
      ? t('dashboard.create_form.name_required')
      : newName.length > MAX_REPO_NAME_LEN
        ? t('dashboard.create_form.name_too_long', { max: MAX_REPO_NAME_LEN })
        : !REPO_NAME_RE.test(newName)
          ? t('dashboard.create_form.name_invalid')
          : ''
  );
  let descError = $derived(newDesc.length > MAX_DESC_LEN ? t('dashboard.create_form.desc_too_long', { max: MAX_DESC_LEN }) : '');
  let canSubmit = $derived(nameError === '' && descError === '');

  // Template options (loaded from API)
  let gitignoreOptions = $state<{ key: string; name: string; description: string }[]>([]);
  let licenseOptions = $state<{ key: string; name: string; description: string }[]>([]);
  let readmeOptions = $state<{ key: string; name: string; description: string }[]>([]);
  let labelSetOptions = $state<{ key: string; name: string; description: string }[]>([]);

  $effect(() => {
    loadTemplates();
  });

  async function loadTemplates() {
    try {
      const [gi, li, re, lb] = await Promise.all([
        repos.templates.gitignores(),
        repos.templates.licenses(),
        repos.templates.readmes(),
        repos.templates.labels(),
      ]);
      gitignoreOptions = gi.data;
      licenseOptions = li.data;
      readmeOptions = re.data;
      labelSetOptions = lb.data;
    } catch (_) {
      // Templates are optional — proceed without them
    }
  }

  async function handleCreate(e: Event) {
    e.preventDefault();
    if (!canSubmit || submitting) return;
    submitting = true;
    try {
      await repos.create({
        name: newName,
        description: newDesc || undefined,
        is_private: newPrivate,
        auto_init: autoInit,
        default_branch: defaultBranch || undefined,
        gitignores: selectedGitignore || undefined,
        license: selectedLicense || undefined,
        readme: autoInit ? selectedReadme : undefined,
        issue_labels: autoInit ? selectedLabels : undefined,
      });
      await onCreated(newName);
    } catch (e) {
      toast.error(toErrorMessage(e, t('errors.save_failed') || 'Create failed'));
    } finally {
      submitting = false;
    }
  }
</script>

<div class="create-form">
  <h2>{t('dashboard.create_form.title')}</h2>
  <form onsubmit={handleCreate}>
    <!-- Repository name -->
    <label>
      {t('dashboard.create_form.name')} <span class="required">*</span>
      <input type="text" bind:value={newName} required maxlength={MAX_REPO_NAME_LEN} placeholder={t('dashboard.create_form.name_placeholder')} />
      {#if nameError}<span class="field-error">{nameError}</span>{/if}
    </label>

    <!-- Description -->
    <label>
      {t('dashboard.create_form.desc')} <span class="optional">{t('common.optional')}</span>
      <input type="text" bind:value={newDesc} maxlength={MAX_DESC_LEN} placeholder={t('common.no_description')} />
      {#if descError}<span class="field-error">{descError}</span>{/if}
    </label>

    <!-- Visibility -->
    <label class="checkbox-label">
      <input type="checkbox" bind:checked={newPrivate} />
      <span>
        <strong>{t('dashboard.create_form.private')}</strong>
        <span class="hint">{t('dashboard.create_form.private_hint')}</span>
      </span>
    </label>

    <hr class="divider" />

    <!-- Auto-initialize -->
    <label class="checkbox-label">
      <input type="checkbox" bind:checked={autoInit} />
      <span>
        <strong>{t('dashboard.create_form.auto_init')}</strong>
        <span class="hint">{t('dashboard.create_form.auto_init_hint')}</span>
      </span>
    </label>

    {#if autoInit}
      <div class="template-section">
        <!-- Default branch -->
        <label>
          {t('dashboard.create_form.default_branch')}
          <input type="text" bind:value={defaultBranch} placeholder="main" />
        </label>

        <!-- .gitignore template -->
        <label>
          {t('dashboard.create_form.gitignore_template')}
          <select bind:value={selectedGitignore}>
            <option value="">{t('dashboard.create_form.none')}</option>
            {#each gitignoreOptions as opt}
              <option value={opt.key}>{opt.name}</option>
            {/each}
          </select>
        </label>

        <!-- LICENSE template -->
        <label>
          {t('dashboard.create_form.license_template')}
          <select bind:value={selectedLicense}>
            <option value="">{t('dashboard.create_form.none')}</option>
            {#each licenseOptions as opt}
              <option value={opt.key}>{opt.name}</option>
            {/each}
          </select>
        </label>

        <!-- README template -->
        <label>
          {t('dashboard.create_form.readme_template')}
          <select bind:value={selectedReadme}>
            {#each readmeOptions as opt}
              <option value={opt.key}>{opt.name}</option>
            {/each}
          </select>
        </label>

        <!-- Default issue labels -->
        <label>
          {t('dashboard.create_form.label_set')}
          <select bind:value={selectedLabels}>
            {#each labelSetOptions as opt}
              <option value={opt.key}>{opt.name}</option>
            {/each}
          </select>
        </label>
      </div>
    {/if}

    <div class="form-actions">
      <button type="submit" class="btn-primary" disabled={!canSubmit || submitting}>{t('dashboard.create_form.submit')}</button>
      <button type="button" class="btn-secondary" onclick={onCancel}>{t('dashboard.create_form.cancel')}</button>
    </div>
  </form>
</div>

<style>
  .create-form {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: 24px;
    margin-bottom: 24px;
  }

  h2 { font-size: 18px; margin-bottom: 16px; }

  form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 13px;
    font-weight: 600;
  }

  .required { color: var(--red); font-weight: 400; }
  .field-error { color: var(--red, #d73a49); font-size: 12px; font-weight: 400; }
  .optional { font-weight: 400; color: var(--text-muted); }

  .checkbox-label {
    flex-direction: row;
    align-items: flex-start;
    gap: 8px;
  }
  .checkbox-label input { width: auto; margin-top: 2px; }

  .hint {
    display: block;
    font-weight: 400;
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 2px;
  }

  .divider {
    border: none;
    border-top: 1px solid var(--border);
    margin: 4px 0;
  }

  .template-section {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding-left: 24px;
    border-left: 2px solid var(--border);
  }

  select {
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 14px;
    background: var(--bg-primary);
    color: var(--text-primary);
  }

  .form-actions {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }

  .btn-primary {
    padding: 6px 16px;
    background: var(--green-dim);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-primary:hover:not(:disabled) { background: var(--green); }
  .btn-primary:disabled { opacity: 0.6; cursor: not-allowed; }

  .btn-secondary {
    padding: 6px 16px;
    background: none;
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 14px;
    cursor: pointer;
  }
  .btn-secondary:hover { background: var(--bg-hover); }
</style>
