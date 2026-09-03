<script lang="ts">
  // Create/edit milestone modal — self-contained: submits through the
  // milestones API and reports success via toast. Inline formError stays
  // inside the modal. due_date uses datetime-local input (local timezone)
  // and is converted to RFC 3339 on submit.
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

  /** RFC 3339 → datetime-local input value (local timezone). */
  function isoToLocalInput(iso: string | null | undefined): string {
    if (!iso) return '';
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return '';
    const pad = (n: number) => String(n).padStart(2, '0');
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }

  function closeByKey(e: KeyboardEvent) {
    if (e.key === 'Escape' || e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      onClose();
    }
  }

  async function handleSave() {
    if (!formData.title.trim()) {
      formError = t('settings.milestone_title_required', 'Title is required');
      return;
    }
    if (formData.title.length > MAX_TITLE_LEN) {
      formError = t('settings.milestone_title_too_long', 'Title is too long');
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

<div
  class="form-overlay"
  onclick={onClose}
  role="button"
  tabindex="0"
  onkeydown={closeByKey}
>
  <div
    class="form-modal"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
  >
    <h2>{milestone ? t('settings.edit_milestone') : t('settings.new_milestone')}</h2>

    {#if formError}
      <div class="error-box">{formError}</div>
    {/if}

    <div class="form-group">
      <label for="milestone-title">{t('settings.milestone_title')}</label>
      <input
        id="milestone-title"
        type="text"
        bind:value={formData.title}
        maxlength={MAX_TITLE_LEN}
        disabled={saving}
      />
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

    <div class="form-actions">
      <button class="btn btn-outline" onclick={onClose} disabled={saving}>
        {t('common.cancel', 'Cancel')}
      </button>
      <button class="btn btn-primary" onclick={handleSave} disabled={saving}>
        {saving
          ? t('common.loading')
          : milestone
            ? t('settings.save_milestone')
            : t('settings.create_milestone')}
      </button>
    </div>
  </div>
</div>

<style>
  .error-box {
    padding: 0.75rem;
    background: rgba(255, 0, 0, 0.1);
    border: 1px solid var(--red, #ff4444);
    border-radius: 6px;
    color: var(--red, #ff4444);
    font-size: 0.9rem;
    margin-bottom: 1rem;
  }

  .form-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .form-modal {
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 2rem;
    max-width: 500px;
    width: 90%;
    max-height: 90vh;
    overflow-y: auto;
  }

  .form-modal h2 {
    margin: 0 0 1.5rem 0;
    color: var(--text-primary);
    font-size: 1.25rem;
  }

  .form-row {
    display: flex;
    gap: 1rem;
  }

  .form-row .form-group {
    flex: 1;
  }

  .form-group {
    margin-bottom: 1.25rem;
  }

  .form-group label {
    display: block;
    margin-bottom: 0.5rem;
    color: var(--text-primary);
    font-weight: 500;
    font-size: 0.9rem;
  }

  .form-group input,
  .form-group select {
    width: 100%;
    padding: 0.6rem 0.75rem;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 0.9rem;
    box-sizing: border-box;
  }

  .form-group input:focus,
  .form-group select:focus {
    outline: none;
    border-color: var(--accent);
  }

  .form-actions {
    display: flex;
    gap: 0.75rem;
    justify-content: flex-end;
    margin-top: 1.5rem;
  }
</style>
