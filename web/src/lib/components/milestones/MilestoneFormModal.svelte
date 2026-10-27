<script lang="ts">
  // Create/edit milestone modal — self-contained: submits through the
  // milestones API and reports success via toast. Inline formError stays
  // inside the modal. due_date uses datetime-local input (local timezone)
  // and is converted to RFC 3339 on submit.
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import Modal from '$lib/components/Modal.svelte';
  import { milestones } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import type { Milestone } from '$lib/types/entities';

  interface Props {
    owner: string;
    repo: string;
    /** Milestone being edited, or null when creating. */
    milestone: Milestone | null;
    onClose: () => void;
    onSaved: () => void;
  }

  let { owner, repo, milestone, onClose, onSaved }: Props = $props();

  const t = createT();

  const MAX_TITLE_LEN = 255;

  // Snapshot props before initialising $state (avoids state_referenced_locally).
  const initialTitle = milestone?.title ?? '';
  const initialDescription = milestone?.description ?? '';
  const initialDueDate = milestone?.due_date ?? null;
  const initialState = milestone?.state ?? 'open';

  let formData = $state({
    title: initialTitle,
    description: initialDescription,
    dueDate: isoToLocalInput(initialDueDate),
    state: initialState,
  });
  let saving = $state(false);
  let formError = $state('');
  let titleError = $state('');

  /** RFC 3339 → datetime-local input value (local timezone). */
  function isoToLocalInput(iso: string | null | undefined): string {
    if (!iso) return '';
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return '';
    const pad = (n: number) => String(n).padStart(2, '0');
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }


  async function handleSave() {
    if (!formData.title.trim()) {
      titleError = t('settings.milestone_title_required', 'Title is required');
      return;
    }
    if (formData.title.length > MAX_TITLE_LEN) {
      titleError = t('settings.milestone_title_too_long', 'Title is too long');
      return;
    }

    try {
      saving = true;
      formError = '';

      const dueIso = formData.dueDate ? new Date(formData.dueDate).toISOString() : null;

      if (milestone) {
        await milestones.update(owner, repo, milestone.id, {
          title: formData.title.trim(),
          description: formData.description.trim() || null,
          state: formData.state,
          // null clears the due date (backend: Some(None) semantics).
          due_date: formData.dueDate ? dueIso : null,
        });
        toast.success(t('settings.save_milestone', 'Milestone saved'));
      } else {
        await milestones.create(owner, repo, {
          title: formData.title.trim(),
          description: formData.description.trim() || undefined,
          due_date: dueIso ?? undefined,
          state: formData.state,
        });
        toast.success(t('settings.create_milestone', 'Milestone created'));
      }

      onClose();
      onSaved();
    } catch (e: unknown) {
      formError = toErrorMessage(e, t('errors.update_failed', 'Update failed'));
    } finally {
      saving = false;
    }
  }
</script>

<Modal
  open={true}
  title={milestone ? t('settings.edit_milestone') : t('settings.new_milestone')}
  submitLabel={saving
    ? t('common.loading')
    : milestone
      ? t('settings.save_milestone')
      : t('settings.create_milestone')}
  busy={saving}
  {onClose}
  onSubmit={handleSave}
>
  {#if formError}
    <ErrorBanner message={formError} />
  {/if}

  <div class="form-group">
    <label for="milestone-title">{t('settings.milestone_title')}</label>
    <input
      id="milestone-title"
      type="text"
      bind:value={formData.title}
      oninput={() => (titleError = '')}
      maxlength={MAX_TITLE_LEN}
      disabled={saving}
      aria-invalid={titleError ? 'true' : undefined}
      aria-describedby={titleError ? 'milestone-title-error' : undefined}
      data-autofocus
    />
    {#if titleError}<p class="field-error" id="milestone-title-error" role="alert">{titleError}</p>{/if}
  </div>

  <div class="form-group">
    <label for="milestone-desc">{t('settings.milestone_desc')}</label>
    <input id="milestone-desc" type="text" bind:value={formData.description} disabled={saving} />
  </div>

  <div class="form-row">
    <div class="form-group">
      <label for="milestone-due">{t('settings.milestone_due')}</label>
      <input id="milestone-due" type="datetime-local" bind:value={formData.dueDate} disabled={saving} />
    </div>
    <div class="form-group">
      <label for="milestone-state">{t('settings.milestone_state')}</label>
      <select id="milestone-state" bind:value={formData.state} disabled={saving}>
        <option value="open">open</option>
        <option value="closed">closed</option>
      </select>
    </div>
  </div>
</Modal>

<style>

  .form-row {
    display: flex;
    gap: 1rem;
  }

  .form-row .form-group {
    flex: 1;
  }

  .form-group input,
  .form-group select {
    width: 100%;
    box-sizing: border-box;
  }

</style>
