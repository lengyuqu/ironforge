<script lang="ts">
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import { tick } from 'svelte';
  import { issues, milestones } from '$lib/api/client.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';
  import type { Milestone } from '$lib/types/entities';

  const t = createT();

  let {
    owner,
    repo,
    initialTitle = '',
    initialBody = '',
    initialLabels = '',
    initialAssignees = '',
    onCreated,
    onCancel,
  }: {
    owner: string;
    repo: string;
    initialTitle?: string;
    initialBody?: string;
    initialLabels?: string;
    initialAssignees?: string;
    onCreated: () => void | Promise<void>;
    onCancel: () => void;
  } = $props();

  // Q6.3: Client-side form validation
  const MAX_TITLE_LEN = 255;
  const MAX_BODY_LEN = 65536;

  let newTitle = $state(initialTitle);
  let newBody = $state(initialBody);
  let newLabels = $state(initialLabels);
  let newAssignees = $state(initialAssignees);
  let openMilestones = $state<Milestone[]>([]);
  let selectedMilestone = $state<number | ''>('');
  let submitting = $state(false);
  let formError = $state('');

  let titleError = $derived(
    newTitle.trim().length === 0
      ? t('issues.create_form.title_required')
      : newTitle.length > MAX_TITLE_LEN
        ? t('issues.create_form.title_too_long', { max: MAX_TITLE_LEN })
        : ''
  );
  let bodyError = $derived(
    newBody.length > MAX_BODY_LEN
      ? t('issues.create_form.body_too_long', { max: MAX_BODY_LEN })
      : ''
  );
  let canSubmit = $derived(titleError === '' && bodyError === '');

  // Errors appear once a field was edited/left or a submit was attempted —
  // never on a pristine form.
  let attempted = $state(false);
  let touched = $state({ title: false, body: false });
  let showTitleError = $derived(titleError !== '' && (attempted || touched.title));
  let showBodyError = $derived(bodyError !== '' && (attempted || touched.body));
  let formEl = $state<HTMLFormElement | null>(null);
  let titleEl = $state<HTMLInputElement | null>(null);

  // The form is revealed by a user action; start typing right away.
  $effect(() => {
    titleEl?.focus();
  });

  $effect(() => {
    loadMilestones();
  });

  async function loadMilestones() {
    try {
      const list = await milestones.list(owner, repo);
      openMilestones = list.filter((m) => m.state === 'open');
    } catch {
      // Degrade gracefully — the selector just stays empty.
      openMilestones = [];
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
    try {
      submitting = true;
      formError = '';
      const labels = newLabels ? newLabels.split(',').map((l) => l.trim()).filter(Boolean) : undefined;
      const assignees = newAssignees
        ? newAssignees.split(',').map((a) => a.trim()).filter(Boolean)
        : undefined;
      await issues.create(
        owner,
        repo,
        newTitle,
        newBody || undefined,
        labels,
        assignees,
        selectedMilestone === '' ? undefined : selectedMilestone,
      );
      await onCreated();
    } catch (err) {
      formError = toErrorMessage(err, t('errors.create_failed', 'Create failed'));
    } finally {
      submitting = false;
    }
  }
</script>

<div class="create-form gh-card">
  <form onsubmit={handleCreate} bind:this={formEl} novalidate>
    <label>
      {t('issues.create_form.title')}
      <input
        type="text"
        bind:value={newTitle}
        required
        maxlength={MAX_TITLE_LEN}
        placeholder={t('issues.create_form.title_placeholder')}
        oninput={() => (touched.title = true)}
        onblur={() => (touched.title = true)}
        aria-invalid={showTitleError ? 'true' : undefined}
        aria-describedby={showTitleError ? 'issue-title-error' : undefined}
        bind:this={titleEl}
      />
      {#if showTitleError}<span class="field-error" id="issue-title-error" role="alert">{titleError}</span>{/if}
    </label>
    <label>
      {t('issues.create_form.body')}
      <span class="optional">{t('issues.create_form.body_hint')}</span>
      <textarea
        bind:value={newBody}
        rows="6"
        maxlength={MAX_BODY_LEN}
        placeholder={t('issues.create_form.body_placeholder')}
        oninput={() => (touched.body = true)}
        onblur={() => (touched.body = true)}
        aria-invalid={showBodyError ? 'true' : undefined}
        aria-describedby={showBodyError ? 'issue-body-error' : undefined}
      ></textarea>
      {#if showBodyError}<span class="field-error" id="issue-body-error" role="alert">{bodyError}</span>{/if}
    </label>
    <label>
      {t('issues.create_form.labels')}
      <span class="optional">{t('issues.create_form.labels_hint')}</span>
      <input
        type="text"
        bind:value={newLabels}
        placeholder={t('issues.create_form.labels_placeholder')}
      />
    </label>
    <label>
      {t('issues.create_form_assignees')}
      <span class="optional">{t('issues.create_form_assignees_hint')}</span>
      <input
        type="text"
        bind:value={newAssignees}
        placeholder={t('issues.create_form_assignees_placeholder')}
      />
    </label>
    <label>
      {t('issues.create_form_milestone')}
      <span class="optional">{t('issues.create_form_milestone_hint')}</span>
      <select bind:value={selectedMilestone}>
        <option value="">{t('issues.create_form_milestone_none')}</option>
        {#each openMilestones as m (m.id)}
          <option value={m.id}>{m.title}</option>
        {/each}
      </select>
    </label>

    {#if formError}
      <ErrorBanner message={formError} />
    {/if}

    <div class="form-actions">
      <button type="submit" class="btn-primary" disabled={submitting}>
        {submitting ? t('common.loading') : t('issues.create_form.submit')}
      </button>
      <button type="button" class="btn-secondary" onclick={onCancel}>
        {t('issues.create_form.cancel')}
      </button>
    </div>
  </form>
</div>

<style>
  .create-form {
    padding: 20px;
    margin-bottom: 24px;
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

  textarea {
    resize: vertical;
  }
</style>
