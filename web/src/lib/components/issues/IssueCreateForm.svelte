<script lang="ts">
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
    if (!canSubmit || submitting) return;
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
    } catch (err: any) {
      formError = toErrorMessage(err, t('errors.create_failed', 'Create failed'));
    } finally {
      submitting = false;
    }
  }
</script>

<div class="create-form gh-card">
  <form onsubmit={handleCreate}>
    <label>
      {t('issues.create_form.title')}
      <input
        type="text"
        bind:value={newTitle}
        required
        maxlength={MAX_TITLE_LEN}
        placeholder={t('issues.create_form.title_placeholder')}
      />
      {#if titleError}<span class="field-error">{titleError}</span>{/if}
    </label>
    <label>
      {t('issues.create_form.body')}
      <span class="optional">{t('issues.create_form.body_hint')}</span>
      <textarea
        bind:value={newBody}
        rows="6"
        maxlength={MAX_BODY_LEN}
        placeholder={t('issues.create_form.body_placeholder')}></textarea>
      {#if bodyError}<span class="field-error">{bodyError}</span>{/if}
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
      <div class="form-error">{formError}</div>
    {/if}

    <div class="form-actions">
      <button type="submit" class="btn-primary" disabled={!canSubmit || submitting}>
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

  .field-error {
    color: var(--red, #d73a49);
    font-size: 12px;
    font-weight: 400;
  }

  textarea {
    font-family: var(--font-mono);
    font-size: 13px;
    resize: vertical;
  }

  select {
    padding: 6px 8px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-primary);
    font-size: 13px;
  }

  .form-error {
    padding: 10px 12px;
    border-radius: 6px;
    font-size: 13px;
    color: var(--red, #d73a49);
    background: rgba(215, 58, 73, 0.08);
    border: 1px solid rgba(215, 58, 73, 0.35);
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
    opacity: 0.6;
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
