<script lang="ts">
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import { milestones, pulls, repos } from '$lib/api/client.svelte';
  import { tick } from 'svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';
  import type { Milestone } from '$lib/types/entities';

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
  let newMilestone = $state<number | ''>('');
  let branches = $state<{ name: string; is_default: boolean }[]>([]);
  let milestoneOptions = $state<Milestone[]>([]);
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
  let headError = $derived(newHead ? '' : t('pulls.create_form.head_required'));
  let canSubmit = $derived(titleError === '' && bodyError === '' && headError === '');

  // Errors appear once a field was edited/left or a submit was attempted —
  // never on a pristine form.
  let attempted = $state(false);
  let touched = $state({ title: false, body: false });
  let showTitleError = $derived(titleError !== '' && (attempted || touched.title));
  let showBodyError = $derived(bodyError !== '' && (attempted || touched.body));
  let showHeadError = $derived(headError !== '' && attempted);
  let formEl = $state<HTMLFormElement | null>(null);

  $effect(() => {
    loadBranches();
    loadMilestones();
  });

  async function loadBranches() {
    try {
      branches = await repos.branches(owner, repo);
    } catch {
      /* branches unavailable — leave selector empty */
    }
  }

  async function loadMilestones() {
    try {
      milestoneOptions = await milestones.list(owner, repo);
    } catch {
      /* milestones unavailable — milestone selector stays hidden */
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

  function onMilestonePick(event: Event) {
    const value = (event.currentTarget as HTMLSelectElement).value;
    newMilestone = value === '' ? '' : Number(value);
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
    try {
      submitting = true;
      error = '';
      await pulls.create(owner, repo, {
        title: newTitle,
        body: newBody || undefined,
        head_branch: newHead,
        base_branch: newBase,
        draft: newDraft,
        ...(newMilestone === '' ? {} : { milestone_id: newMilestone }),
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
    <ErrorBanner message={error} />
  {/if}

  <form onsubmit={handleCreate} bind:this={formEl} novalidate>
    <div class="branch-row">
      <label>
        {t('pulls.create_form.from')}
        <select
          bind:value={newHead}
          required
          aria-invalid={showHeadError ? 'true' : undefined}
          aria-describedby={showHeadError ? 'pr-head-error' : undefined}
        >
          <option value="" disabled selected>{t('pulls.create_form.select_branch')}</option>
          {#each branches as b (b.name)}
            <option value={b.name}>{b.name}</option>
          {/each}
        </select>
        {#if showHeadError}<span class="field-error" id="pr-head-error" role="alert">{headError}</span>{/if}
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
      {t('pulls.create_form.title_label')}
      <input
        type="text"
        bind:value={newTitle}
        required
        maxlength={MAX_TITLE_LEN}
        placeholder={t('pulls.create_form.title_placeholder')}
        oninput={() => (touched.title = true)}
        onblur={() => (touched.title = true)}
        aria-invalid={showTitleError ? 'true' : undefined}
        aria-describedby={showTitleError ? 'pr-title-error' : undefined}
      />
      {#if showTitleError}<span class="field-error" id="pr-title-error" role="alert">{titleError}</span>{/if}
    </label>
    <label>
      {t('pulls.create_form.description')} <span class="optional">{t('pulls.create_form.description_hint')}</span>
      <textarea
        bind:value={newBody}
        rows="4"
        maxlength={MAX_BODY_LEN}
        placeholder={t('pulls.create_form.description_placeholder')}
        oninput={() => (touched.body = true)}
        onblur={() => (touched.body = true)}
        aria-invalid={showBodyError ? 'true' : undefined}
        aria-describedby={showBodyError ? 'pr-body-error' : undefined}
      ></textarea>
      {#if showBodyError}<span class="field-error" id="pr-body-error" role="alert">{bodyError}</span>{/if}
    </label>
    {#if milestoneOptions.length > 0}
      <label>
        {t('pulls.create_form.milestone')} <span class="optional">({t('issues.create_form_milestone_hint')})</span>
        <select onchange={onMilestonePick}>
          <option value="">{t('issues.create_form_milestone_none')}</option>
          {#each milestoneOptions as ms (ms.id)}
            <option value={ms.id} selected={newMilestone === ms.id}>{ms.title}</option>
          {/each}
        </select>
      </label>
    {/if}
    <label class="draft-option">
      <input type="checkbox" bind:checked={newDraft} />
      <span>{t('pulls.create_form.draft')}</span>
    </label>
    <div class="form-actions">
      <button type="submit" class="btn-primary" disabled={submitting}>
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

</style>
