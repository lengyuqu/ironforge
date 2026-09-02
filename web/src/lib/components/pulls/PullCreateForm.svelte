<script lang="ts">
  import { pulls, repos } from '$lib/api/client.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    repo,
    onCreated,
    onCancel,
  }: {
    owner: string;
    repo: string;
    onCreated: () => void | Promise<void>;
    onCancel: () => void;
  } = $props();

  // Q6.3: Client-side form validation
  const MAX_TITLE_LEN = 255;
  const MAX_BODY_LEN = 65536;

  let newTitle = $state('');
  let newBody = $state('');
  let newHead = $state('');
  let newBase = $state('main');
  let newDraft = $state(false);
  let branches = $state<{ name: string; is_default: boolean }[]>([]);
  let templateLoaded = $state(false);
  let submitting = $state(false);
  let error = $state('');

  let titleError = $derived(
    newTitle.trim().length === 0
      ? t('pulls.create_form.title_required')
      : newTitle.length > MAX_TITLE_LEN
        ? t('pulls.create_form.title_too_long', { max: MAX_TITLE_LEN })
        : ''
  );
  let bodyError = $derived(newBody.length > MAX_BODY_LEN ? t('pulls.create_form.body_too_long', { max: MAX_BODY_LEN }) : '');
  let canSubmit = $derived(titleError === '' && bodyError === '' && !!newHead && !submitting);

  $effect(() => {
    loadBranches();
  });

  async function loadBranches() {
    try {
      branches = await repos.branches(owner, repo);
    } catch {
      /* branches unavailable — leave selector empty */
    }
  }

  async function preloadTemplate() {
    if (templateLoaded) return;
    try {
      const template = await pulls.template(owner, repo);
      if (template?.content && !newBody) newBody = template.content;
    } catch {
      /* template unavailable — fall back to empty body */
    } finally {
      templateLoaded = true;
    }
  }

  async function handleCreate(e: Event) {
    e.preventDefault();
    if (!canSubmit) return;
    try {
      submitting = true;
      error = '';
      await pulls.create(owner, repo, {
        title: newTitle,
        body: newBody || undefined,
        head_branch: newHead,
        base_branch: newBase,
        draft: newDraft,
      });
      await onCreated();
    } catch (err: any) {
      error = toErrorMessage(err, t('errors.create_failed', 'Create failed'));
    } finally {
      submitting = false;
    }
  }
</script>

<div class="create-form gh-card">
  <h2>{t('pulls.create_form.title')}</h2>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <form onsubmit={handleCreate}>
    <div class="branch-row">
      <label>
        {t('pulls.create_form.from')}
        <select bind:value={newHead} required>
          <option value="" disabled selected>{t('pulls.create_form.select_branch')}</option>
          {#each branches as b (b.name)}
            <option value={b.name}>{b.name}</option>
          {/each}
        </select>
      </label>
      <span class="arrow">→</span>
      <label>
        {t('pulls.create_form.into')}
        <select bind:value={newBase} required>
          {#each branches as b (b.name)}
            <option value={b.name}>{b.name} {b.is_default ? t('repo.browser.default_branch') : ''}</option>
          {/each}
        </select>
      </label>
    </div>
    <label>
      {t('pulls.create_form.description')}
      <input
        type="text"
        bind:value={newTitle}
        required
        maxlength={MAX_TITLE_LEN}
        placeholder={t('pulls.create_form.description_placeholder')}
      />
      {#if titleError}<span class="field-error">{titleError}</span>{/if}
    </label>
    <label>
      {t('pulls.create_form.description')} <span class="optional">{t('pulls.create_form.description_hint')}</span>
      <textarea
        bind:value={newBody}
        rows="4"
        maxlength={MAX_BODY_LEN}
        placeholder={t('pulls.create_form.description_placeholder')}
      ></textarea>
      {#if bodyError}<span class="field-error">{bodyError}</span>{/if}
    </label>
    <label class="draft-option">
      <input type="checkbox" bind:checked={newDraft} />
      <span>{t('pulls.create_form.draft')}</span>
    </label>
    <div class="form-actions">
      <button type="submit" class="btn-primary" disabled={!canSubmit}>
        {submitting ? t('common.loading') : t('pulls.create_form.submit')}
      </button>
      <button type="button" class="btn-secondary" onclick={onCancel}>{t('pulls.create_form.cancel')}</button>
    </div>
  </form>
</div>

<style>
  h2 {
    font-size: 18px;
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

  .optional {
    font-weight: 400;
    color: var(--text-muted);
  }

  .field-error {
    color: var(--red, #d73a49);
    font-size: 12px;
    font-weight: 400;
  }

  .draft-option {
    flex-direction: row;
    align-items: center;
    font-weight: 500;
  }

  .draft-option input {
    width: auto;
  }

  select {
    padding: 6px 10px;
  }

  textarea {
    font-family: var(--font-mono);
    font-size: 13px;
    resize: vertical;
  }

  .branch-row {
    display: flex;
    align-items: flex-end;
    gap: 12px;
  }

  .arrow {
    font-size: 20px;
    color: var(--text-muted);
    margin-bottom: 8px;
  }

  .form-actions {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }

  .btn-primary {
    padding: 6px 16px;
    background: var(--accent);
    color: #fff;
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }

  .btn-primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-secondary {
    padding: 6px 16px;
    background: none;
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 14px;
    cursor: pointer;
  }

  .error-banner {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 10px 12px;
    border-radius: var(--radius);
    margin-bottom: 12px;
  }
</style>
