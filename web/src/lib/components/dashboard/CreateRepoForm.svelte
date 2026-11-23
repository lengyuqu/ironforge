<script lang="ts">
  import { tick } from 'svelte';
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
  /** 0 = start empty; otherwise the id of a repository to copy. */
  let selectedTemplate = $state(0);
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

  // Errors appear once a field was edited/left or a submit was attempted —
  // never on a pristine form.
  let attempted = $state(false);
  let touched = $state({ name: false, desc: false });
  let showNameError = $derived(nameError !== '' && (attempted || touched.name));
  let showDescError = $derived(descError !== '' && (attempted || touched.desc));
  let formEl = $state<HTMLFormElement | null>(null);
  let nameEl = $state<HTMLInputElement | null>(null);

  // The form is revealed by a user action; start typing right away.
  $effect(() => {
    nameEl?.focus();
  });

  // Template options (loaded from API)
  let gitignoreOptions = $state<{ key: string; name: string; description: string }[]>([]);
  let licenseOptions = $state<{ key: string; name: string; description: string }[]>([]);
  let readmeOptions = $state<{ key: string; name: string; description: string }[]>([]);
  let labelSetOptions = $state<{ key: string; name: string; description: string }[]>([]);
  let templateOptions = $state<
    { id: number; name: string; description: string | null; is_private: boolean }[]
  >([]);

  $effect(() => {
    loadTemplates();
  });

  async function loadTemplates() {
    try {
      const [gi, li, re, lb, tpl] = await Promise.all([
        repos.templates.gitignores(),
        repos.templates.licenses(),
        repos.templates.readmes(),
        repos.templates.labels(),
        // Optional: an instance may have no templates at all.
        repos.availableTemplates().catch(() => ({ data: [] })),
      ]);
      gitignoreOptions = gi.data;
      licenseOptions = li.data;
      readmeOptions = re.data;
      labelSetOptions = lb.data;
      templateOptions = tpl.data;
    } catch (_) {
      // Templates are optional — proceed without them
    }
  }

  async function handleCreate(e: Event) {
    e.preventDefault();
    if (submitting) return;
    if (!canSubmit) {
      attempted = true;
      await tick();
      formEl?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
      return;
    }
    submitting = true;
    try {
      // A template carries its own files, so the generated ones are not sent:
      // the server ignores them anyway, and sending them would suggest a
      // combination that does not exist.
      const fromTemplate = selectedTemplate > 0;
      await repos.create({
        name: newName,
        description: newDesc || undefined,
        is_private: newPrivate,
        auto_init: fromTemplate ? false : autoInit,
        default_branch: defaultBranch || undefined,
        gitignores: fromTemplate || !autoInit ? undefined : selectedGitignore || undefined,
        license: fromTemplate || !autoInit ? undefined : selectedLicense || undefined,
        readme: !fromTemplate && autoInit ? selectedReadme : undefined,
        issue_labels: !fromTemplate && autoInit ? selectedLabels : undefined,
        template_repo_id: fromTemplate ? selectedTemplate : undefined,
      });
      await onCreated(newName);
    } catch (e) {
      toast.error(toErrorMessage(e, t('errors.create_failed')));
    } finally {
      submitting = false;
    }
  }
</script>

<div class="create-form surface">
  <h2>{t('dashboard.create_form.title')}</h2>
  <form onsubmit={handleCreate} bind:this={formEl} novalidate>
    <!-- Repository name -->
    <label>
      {t('dashboard.create_form.name')} <span class="required">*</span>
      <input
        type="text"
        bind:value={newName}
        required
        maxlength={MAX_REPO_NAME_LEN}
        placeholder={t('dashboard.create_form.name_placeholder')}
        oninput={() => (touched.name = true)}
        onblur={() => (touched.name = true)}
        aria-invalid={showNameError ? 'true' : undefined}
        aria-describedby={showNameError ? 'repo-name-error' : undefined}
        bind:this={nameEl}
      />
      {#if showNameError}<span class="field-error" id="repo-name-error" role="alert">{nameError}</span>{/if}
    </label>

    <!-- Description -->
    <label>
      {t('dashboard.create_form.desc')} <span class="optional">{t('common.optional')}</span>
      <input
        type="text"
        bind:value={newDesc}
        maxlength={MAX_DESC_LEN}
        placeholder={t('common.no_description')}
        oninput={() => (touched.desc = true)}
        onblur={() => (touched.desc = true)}
        aria-invalid={showDescError ? 'true' : undefined}
        aria-describedby={showDescError ? 'repo-desc-error' : undefined}
      />
      {#if showDescError}<span class="field-error" id="repo-desc-error" role="alert">{descError}</span>{/if}
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

    <!-- Start from a template (only when the instance has any) -->
    {#if templateOptions.length > 0}
      <label>
        {t('dashboard.create_form.template')}
        <select bind:value={selectedTemplate}>
          <option value={0}>{t('dashboard.create_form.template_none')}</option>
          {#each templateOptions as template (template.id)}
            <option value={template.id}>
              {template.name}{template.is_private ? ' 🔒' : ''}
            </option>
          {/each}
        </select>
        <span class="hint">{t('dashboard.create_form.template_hint')}</span>
      </label>
    {/if}

    <!-- Auto-initialize -->
    <label class="checkbox-label" class:disabled={selectedTemplate > 0}>
      <input type="checkbox" bind:checked={autoInit} disabled={selectedTemplate > 0} />
      <span>
        <strong>{t('dashboard.create_form.auto_init')}</strong>
        <span class="hint">{t('dashboard.create_form.auto_init_hint')}</span>
      </span>
    </label>

    {#if autoInit && selectedTemplate === 0}
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
      <button type="submit" class="btn-primary" disabled={submitting}>{t('dashboard.create_form.submit')}</button>
      <button type="button" class="btn-secondary" onclick={onCancel}>{t('dashboard.create_form.cancel')}</button>
    </div>
  </form>
</div>

<style>
  .create-form {
    padding: 24px;
    margin-bottom: 24px;
  }

  h2 {
    margin-bottom: 16px;
  }

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
  .optional { font-weight: 400; color: var(--text-muted); }

  .checkbox-label {
    flex-direction: row;
  }
  .checkbox-label input { width: auto; margin-top: 2px; }

  .hint {
    display: block;
    font-weight: 400;
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 2px;
  }

  .checkbox-label.disabled { opacity: 0.5; }

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
</style>
