<script lang="ts">
  // Escalation reminders for a repository. Repository admins change them; anyone
  // who may read the collaboration data sees them. Saving is versioned: a stale
  // `expected_version` is a VERSION_CONFLICT and the stored copy is re-read.
  //
  // What this does and does not do is stated on the page, because "escalation"
  // sounds like paging a person and it is not: a reminder is a notice for the
  // responsible agent SESSION, produced lazily when that session reads its
  // notices or inbox. Nobody is e-mailed and no timer runs.
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { ApiError } from '$lib/api/_base.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import Button from '$lib/components/Button.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import SectionCard from './SectionCard.svelte';
  import { collabOps, type EscalationSettings } from '$lib/api/collabOps';
  import { formatTime } from '$lib/utils/collaboration';
  import { buildEscalation, draftFromSettings, isVersionConflict, minuteToClock, type EscalationDraft } from '$lib/utils/collabOps';

  interface Props {
    owner: string;
    repo: string;
    /** Another admin-only read already said this viewer is not an admin. */
    knownNotAdmin?: boolean;
  }
  let { owner, repo, knownNotAdmin = false }: Props = $props();
  const t = createT();

  let settings = $state<EscalationSettings | null>(null);
  let loading = $state(true);
  let loadError = $state('');
  let editing = $state(false);
  let draft = $state<EscalationDraft | null>(null);
  let busy = $state(false);
  let saveError = $state('');
  let notice = $state('');
  /** Set once the server refused a write for lack of admin rights. */
  let refusedWrite = $state(false);
  let notAdmin = $derived(refusedWrite || knownNotAdmin);
  let fieldProblem = $state<string>('');

  async function load() {
    loading = true;
    loadError = '';
    try {
      settings = await collabOps.getEscalation(owner, repo);
    } catch (e) {
      settings = null;
      loadError = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void owner;
    void repo;
    editing = false;
    refusedWrite = false;
    void load();
  });

  function startEdit() {
    if (!settings) return;
    draft = draftFromSettings(settings);
    saveError = '';
    notice = '';
    fieldProblem = '';
    editing = true;
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (!settings || !draft || busy) return;
    const built = buildEscalation(draft, settings.version);
    if (!built.ok) {
      fieldProblem = t(`collab_ops.escalation.problem.${built.problem.code}`);
      return;
    }
    fieldProblem = '';
    busy = true;
    saveError = '';
    notice = '';
    try {
      settings = await collabOps.putEscalation(owner, repo, built.input);
      editing = false;
      toast.success(t('collab_ops.escalation.saved'));
    } catch (err) {
      saveError = toErrorMessage(err, t('common.unknown_error'));
      if (err instanceof ApiError && err.status === 403) {
        refusedWrite = true;
        editing = false;
      } else if (err instanceof ApiError && isVersionConflict(err.code)) {
        // Someone else saved first: show what is stored now, keep nothing stale.
        await load();
        editing = false;
        notice = t('collab_ops.escalation.conflict_reloaded');
      }
    } finally {
      busy = false;
    }
  }

  const mins = (secs: number) => t('collab_ops.escalation.minutes', { n: Math.round(secs / 60) });
</script>

<SectionCard title={t('collab_ops.escalation.title')} note={t('collab_ops.escalation.note')} {loading} error={loadError}>
  {#if settings}
    {#if notice}<p class="info" role="status" data-testid="escalation-conflict">{notice}</p>{/if}
    {#if saveError}<ErrorBanner message={saveError} />{/if}
    {#if notAdmin}
      <p class="info" role="status" data-testid="escalation-not-admin">{t('collab_ops.escalation.admin_only')}</p>
    {/if}

    {#if !editing}
      <dl class="kv" data-testid="escalation-view">
        <dt>{t('collab_ops.escalation.enabled')}</dt>
        <dd>{settings.enabled ? t('collab_ops.escalation.on') : t('collab_ops.escalation.off')}</dd>
        <dt>{t('collab_ops.escalation.blocked_after')}</dt>
        <dd>{mins(settings.blocked_after_secs)}</dd>
        <dt>{t('collab_ops.escalation.question_after')}</dt>
        <dd>{mins(settings.question_after_secs)}</dd>
        <dt>{t('collab_ops.escalation.handoff_after')}</dt>
        <dd>{mins(settings.handoff_after_secs)}</dd>
        <dt>{t('collab_ops.escalation.repeat_after')}</dt>
        <dd>{settings.repeat_after_secs === 0 ? t('collab_ops.escalation.repeat_once') : mins(settings.repeat_after_secs)}</dd>
        <dt>{t('collab_ops.escalation.quiet')}</dt>
        <dd>
          {#if settings.quiet_hours}
            {minuteToClock(settings.quiet_hours.start_minute)}–{minuteToClock(settings.quiet_hours.end_minute)} {settings.timezone}
          {:else}
            {t('collab_ops.escalation.quiet_none')}
          {/if}
        </dd>
        <dt>{t('collab_ops.escalation.stored')}</dt>
        <dd>
          {#if settings.source === 'default'}
            {t('collab_ops.escalation.default_source')}
          {:else}
            {t('collab_ops.escalation.version_meta', { version: settings.version, time: formatTime(settings.updated_at), user: settings.updated_by_user_id ?? '?' })}
          {/if}
        </dd>
      </dl>
      <Button size="sm" onclick={startEdit} disabled={notAdmin}>{t('collab_ops.escalation.edit')}</Button>
    {:else if draft}
      <form class="eform" onsubmit={save} data-testid="escalation-form">
        <label class="chk">
          <input type="checkbox" bind:checked={draft.enabled} />
          {t('collab_ops.escalation.enabled')}
        </label>
        <div class="grid">
          <label>{t('collab_ops.escalation.blocked_after')}
            <input type="text" inputmode="numeric" bind:value={draft.blockedMinutes} aria-label={t('collab_ops.escalation.blocked_after')} />
          </label>
          <label>{t('collab_ops.escalation.question_after')}
            <input type="text" inputmode="numeric" bind:value={draft.questionMinutes} aria-label={t('collab_ops.escalation.question_after')} />
          </label>
          <label>{t('collab_ops.escalation.handoff_after')}
            <input type="text" inputmode="numeric" bind:value={draft.handoffMinutes} aria-label={t('collab_ops.escalation.handoff_after')} />
          </label>
          <label>{t('collab_ops.escalation.repeat_after')}
            <input type="text" inputmode="numeric" bind:value={draft.repeatMinutes} aria-label={t('collab_ops.escalation.repeat_after')} />
          </label>
        </div>
        <p class="hint">{t('collab_ops.escalation.units_hint')}</p>
        <label class="chk">
          <input type="checkbox" bind:checked={draft.quiet} />
          {t('collab_ops.escalation.quiet')}
        </label>
        {#if draft.quiet}
          <div class="grid">
            <label>{t('collab_ops.escalation.quiet_start')}
              <input type="time" bind:value={draft.quietStart} aria-label={t('collab_ops.escalation.quiet_start')} />
            </label>
            <label>{t('collab_ops.escalation.quiet_end')}
              <input type="time" bind:value={draft.quietEnd} aria-label={t('collab_ops.escalation.quiet_end')} />
            </label>
          </div>
        {/if}
        {#if fieldProblem}<p class="problem" role="alert" data-testid="escalation-problem">{fieldProblem}</p>{/if}
        <div class="buttons">
          <Button type="submit" variant="primary" disabled={busy}>{busy ? t('common.loading') : t('collab_ops.escalation.save')}</Button>
          <Button onclick={() => (editing = false)} disabled={busy}>{t('common.cancel')}</Button>
        </div>
      </form>
    {/if}

    <div class="explain">
      <h3>{t('collab_ops.escalation.what_title')}</h3>
      <ul>
        <li>{t('collab_ops.escalation.what_1')}</li>
        <li>{t('collab_ops.escalation.what_2')}</li>
        <li>{t('collab_ops.escalation.what_3')}</li>
        <li>{t('collab_ops.escalation.what_4')}</li>
      </ul>
    </div>
  {/if}
</SectionCard>

<style>
  .kv { display: grid; grid-template-columns: max-content 1fr; gap: 4px 16px; margin: 0 0 12px; font-size: 13px; }
  dt { color: var(--text-secondary); font-weight: 600; }
  dd { margin: 0; }
  .info { margin: 0 0 10px; padding: 8px 12px; border-left: 3px solid var(--yellow, #d29922); background: var(--bg-secondary); font-size: 13px; }
  .eform { display: flex; flex-direction: column; gap: 12px; margin-bottom: 12px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; }
  .grid label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: var(--text-secondary); }
  .chk { display: flex; align-items: center; gap: 8px; font-size: 13px; }
  .hint { margin: 0; font-size: 12px; color: var(--text-muted); }
  .problem { margin: 0; font-size: 12px; color: var(--red); }
  .buttons { display: flex; gap: 8px; }
  .explain { margin-top: 16px; padding-top: 12px; border-top: 1px solid var(--border-light); font-size: 12px; color: var(--text-secondary); }
  .explain h3 { margin: 0 0 6px; font-size: 13px; }
  .explain ul { margin: 0; padding-left: 18px; }
</style>
