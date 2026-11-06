<script lang="ts">
  // Create a task template, or save an edit of one as a NEW immutable version.
  // Editing never changes older versions or the tasks already made from them.
  // Everything typed here is stored text; the server validates it again.
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { ApiError } from '$lib/api/_base.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import Button from '$lib/components/Button.svelte';
  import FormField from '$lib/components/FormField.svelte';
  import { collabOps, type TemplateView } from '$lib/api/collabOps';
  import { newClientId } from '$lib/utils/collaboration';
  import {
    LIMITS,
    checklistToText,
    linesOf,
    placeholdersOf,
    sameContent,
    templateProblem,
    textToChecklist,
    isVersionConflict,
  } from '$lib/utils/collabOps';

  interface Props {
    owner: string;
    repo: string;
    /** The template being edited (its latest version is the starting point); null to create. */
    template: TemplateView | null;
    onSaved: (saved: TemplateView) => void;
    onCancel: () => void;
    /** The template changed under the editor: the parent must reload it. */
    onConflict?: () => void;
  }

  let { owner, repo, template, onSaved, onCancel, onConflict }: Props = $props();
  const t = createT();

  // The draft starts from the version the editor saw; `expected` pins that number.
  const start = template?.version ?? null;
  const expected = template?.latest_version ?? 0;
  let name = $state(template?.name ?? '');
  let titlePattern = $state(start?.title_pattern ?? '');
  let goal = $state(start?.goal ?? '');
  let deliverables = $state((start?.deliverables ?? []).join('\n'));
  let checklist = $state(checklistToText(start?.checklist ?? []));

  let key = $state(newClientId());
  let busy = $state(false);
  let error = $state('');
  let conflict = $state(false);

  let content = $derived({
    title_pattern: titlePattern,
    goal,
    deliverables: linesOf(deliverables),
    checklist: textToChecklist(checklist),
  });
  let problem = $derived(templateProblem(content, template ? undefined : name));
  let unchanged = $derived(start !== null && sameContent(content, start));
  let params = $derived(placeholdersOf(titlePattern) ?? []);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (busy || problem || unchanged) return;
    busy = true;
    error = '';
    conflict = false;
    try {
      const saved = template
        ? await collabOps.newTemplateVersion(owner, repo, template.id, {
            client_version_id: key,
            expected_version: expected,
            ...content,
          })
        : await collabOps.createTemplate(owner, repo, { client_template_id: key, name: name.trim(), ...content });
      toast.success(template ? t('collab_ops.templates.version_saved') : t('collab_ops.templates.created'));
      onSaved(saved);
    } catch (err) {
      error = toErrorMessage(err, t('common.unknown_error'));
      if (err instanceof ApiError && isVersionConflict(err.code)) conflict = true;
      // A new attempt after a failure is a new request, not a replay of the old one.
      key = newClientId();
    } finally {
      busy = false;
    }
  }
</script>

<form class="tform" onsubmit={submit} data-testid="template-form">
  <h3>
    {template
      ? t('collab_ops.templates.edit_title', { name: template.name, version: expected + 1 })
      : t('collab_ops.templates.create_title')}
  </h3>
  {#if template}<p class="hint">{t('collab_ops.templates.edit_hint', { version: expected })}</p>{/if}

  {#if !template}
    <FormField label={t('collab_ops.templates.name')} required hint={t('collab_ops.templates.name_hint', { max: LIMITS.nameChars })}>
      {#snippet children({ id, describedBy })}
        <input {id} aria-describedby={describedBy} type="text" bind:value={name} maxlength="400" />
      {/snippet}
    </FormField>
  {/if}

  <FormField label={t('collab_ops.templates.title_pattern')} required hint={t('collab_ops.templates.title_pattern_hint', { max: LIMITS.renderedTitleChars })}>
    {#snippet children({ id, describedBy })}
      <input {id} aria-describedby={describedBy} type="text" bind:value={titlePattern} class="mono" />
    {/snippet}
  </FormField>
  {#if params.length > 0}
    <p class="hint">{t('collab_ops.templates.params_found')} {params.join(', ')}</p>
  {/if}

  <FormField label={t('collab_ops.templates.goal')} required hint={t('collab_ops.templates.goal_hint', { max: LIMITS.goalBytes })}>
    {#snippet children({ id, describedBy })}
      <textarea {id} aria-describedby={describedBy} rows="4" bind:value={goal}></textarea>
    {/snippet}
  </FormField>

  <FormField label={t('collab_ops.templates.deliverables')} optional hint={t('collab_ops.templates.deliverables_hint', { max: LIMITS.deliverables })}>
    {#snippet children({ id, describedBy })}
      <textarea {id} aria-describedby={describedBy} rows="3" bind:value={deliverables}></textarea>
    {/snippet}
  </FormField>

  <FormField label={t('collab_ops.templates.checklist')} optional hint={t('collab_ops.templates.checklist_hint', { max: LIMITS.checklist })}>
    {#snippet children({ id, describedBy })}
      <textarea {id} aria-describedby={describedBy} rows="4" bind:value={checklist}></textarea>
    {/snippet}
  </FormField>

  {#if problem && (name.trim() || titlePattern || goal || template)}
    <p class="problem" role="status" data-testid="template-problem">{t(`collab_ops.templates.problem.${problem}`)}</p>
  {:else if unchanged}
    <p class="hint" data-testid="template-unchanged">{t('collab_ops.templates.unchanged')}</p>
  {/if}
  {#if error}<ErrorBanner message={error} />{/if}
  {#if conflict}
    <p class="hint" data-testid="template-conflict">
      {t('collab_ops.templates.conflict_hint')}
      {#if onConflict}<Button size="sm" onclick={() => onConflict?.()}>{t('collab_ops.templates.reload')}</Button>{/if}
    </p>
  {/if}

  <div class="buttons">
    <Button type="submit" variant="primary" disabled={busy || problem !== null || unchanged}>
      {busy ? t('common.loading') : template ? t('collab_ops.templates.save_version') : t('collab_ops.templates.create')}
    </Button>
    <Button onclick={onCancel} disabled={busy}>{t('common.cancel')}</Button>
  </div>
</form>

<style>
  .tform { display: flex; flex-direction: column; gap: 12px; margin-bottom: 16px; padding: 12px; border: 1px solid var(--border); border-radius: 6px; }
  h3 { margin: 0; font-size: 14px; }
  .hint { margin: 0; color: var(--text-muted); font-size: 12px; }
  .problem { margin: 0; color: var(--red); font-size: 12px; }
  .buttons { display: flex; gap: 8px; }
  input.mono { font-family: var(--font-mono); }
  textarea, input[type='text'] { width: 100%; }
</style>
