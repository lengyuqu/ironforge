<script lang="ts">
  // Budgets and reported usage of one task.
  //
  // Informational bookkeeping reported by clients, not measured by the
  // platform: every figure carries its exact unit and source, amounts of
  // different units or sources are never added or converted, missing usage is
  // shown as UNKNOWN (never zero), and a threshold only informs — nothing is
  // paused, blocked or stopped.
  //
  // The server has a user door for both writes (set a budget, report usage), so
  // the two small forms below act as the signed-in account (no session
  // credentials). A person's usage report is a claim like any other.
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { toast } from '$lib/components/toast.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import Button from '$lib/components/Button.svelte';
  import StatusBadge from './StatusBadge.svelte';
  import {
    collabRelations,
    type BudgetEntry,
    type BudgetSummary,
    type UsageRecord,
  } from '$lib/api/collabRelations';
  import { formatTime, newClientId } from '$lib/utils/collaboration';
  import { errorText, isValidAmount, isValidToken, localToIso, mustReload } from '$lib/utils/collabRelations';

  interface Props {
    owner: string;
    repo: string;
    taskId: string;
  }

  let { owner, repo, taskId }: Props = $props();
  const t = createT();
  const readMsg = (e: unknown) => toErrorMessage(e, t('common.unknown_error'));

  let summary = $state<BudgetSummary | null>(null);
  let loading = $state(true);
  let error = $state('');

  async function load() {
    error = '';
    try {
      summary = await collabRelations.getBudget(owner, repo, taskId);
    } catch (e) {
      error = readMsg(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void owner;
    void repo;
    void taskId;
    loading = true;
    summary = null;
    showRecords = false;
    records = [];
    setOpen = false;
    usageOpen = false;
    void load();
  });

  // ── reported usage records ──
  let showRecords = $state(false);
  let records = $state<UsageRecord[]>([]);
  let recordsCursor = $state<string | null>(null);
  let recordsLoading = $state(false);
  let recordsError = $state('');

  async function loadRecords(append: boolean) {
    recordsLoading = true;
    recordsError = '';
    try {
      const page = await collabRelations.listUsage(owner, repo, taskId, {
        limit: 25,
        cursor: append ? recordsCursor : null,
      });
      records = append ? [...records, ...page.items] : page.items;
      recordsCursor = page.next_cursor;
    } catch (e) {
      recordsError = readMsg(e);
    } finally {
      recordsLoading = false;
    }
  }

  function toggleRecords() {
    showRecords = !showRecords;
    if (showRecords) void loadRecords(false);
  }

  // ── set / replace a budget ──
  let setOpen = $state(false);
  let sUnit = $state('');
  let sSource = $state('');
  let sAmount = $state('');
  let sAlert = $state('');
  let sPeriod = $state('');
  let setBusy = $state(false);
  let setError = $state('');

  let matching = $derived(
    summary?.budgets.find((b) => b.unit === sUnit && b.source === sSource) ?? null,
  );
  let setProblems = $derived({
    unit: sUnit !== '' && !isValidToken(sUnit, 32),
    source: sSource !== '' && !isValidToken(sSource, 64),
    amount: sAmount !== '' && !isValidAmount(sAmount),
    alert: sAlert !== '' && !isValidAmount(sAlert),
  });
  let setReady = $derived(
    isValidToken(sUnit, 32) &&
      isValidToken(sSource, 64) &&
      isValidAmount(sAmount) &&
      (sAlert === '' || isValidAmount(sAlert)) &&
      sPeriod.trim() !== '',
  );

  function openSet(from?: BudgetEntry) {
    setOpen = true;
    usageOpen = false;
    setError = '';
    sUnit = from?.unit ?? '';
    sSource = from?.source ?? '';
    sAmount = from?.amount ?? '';
    sAlert = from?.alert_at ?? '';
    sPeriod = from?.sampling_period ?? '';
  }

  async function submitSet(e: SubmitEvent) {
    e.preventDefault();
    if (setBusy || !setReady) return;
    setBusy = true;
    setError = '';
    try {
      await collabRelations.setBudget(owner, repo, taskId, {
        expected_version: matching?.version ?? 0,
        unit: sUnit,
        source: sSource,
        amount: sAmount.trim(),
        alert_at: sAlert.trim() === '' ? null : sAlert.trim(),
        sampling_period: sPeriod,
      });
      toast.success(t('collab_relations.budget.set_done'));
      setOpen = false;
      await load();
    } catch (err) {
      setError = errorText('budget', err, t);
      if (mustReload(err)) {
        setError = `${setError} ${t('collab_relations.budget.set_reloaded')}`;
        await load();
      }
    } finally {
      setBusy = false;
    }
  }

  // ── report usage ──
  let usageOpen = $state(false);
  let uUnit = $state('');
  let uSource = $state('');
  let uAmount = $state('');
  let uStart = $state('');
  let uEnd = $state('');
  let usageBusy = $state(false);
  let usageError = $state('');
  let usageKey = $state(newClientId());

  let usageProblems = $derived({
    unit: uUnit !== '' && !isValidToken(uUnit, 32),
    source: uSource !== '' && !isValidToken(uSource, 64),
    amount: uAmount !== '' && !isValidAmount(uAmount),
  });
  let usageReady = $derived(
    isValidToken(uUnit, 32) &&
      isValidToken(uSource, 64) &&
      isValidAmount(uAmount) &&
      localToIso(uStart) !== null &&
      localToIso(uEnd) !== null,
  );

  function openUsage() {
    usageOpen = true;
    setOpen = false;
    usageError = '';
    uUnit = '';
    uSource = '';
    uAmount = '';
    uStart = '';
    uEnd = '';
    usageKey = newClientId();
  }

  async function submitUsage(e: SubmitEvent) {
    e.preventDefault();
    if (usageBusy || !usageReady) return;
    usageBusy = true;
    usageError = '';
    try {
      await collabRelations.reportUsage(owner, repo, taskId, {
        client_usage_id: usageKey,
        unit: uUnit,
        source: uSource,
        amount: uAmount.trim(),
        period_start: localToIso(uStart)!,
        period_end: localToIso(uEnd)!,
      });
      toast.success(t('collab_relations.budget.usage_done'));
      usageOpen = false;
      usageKey = newClientId();
      await load();
      if (showRecords) await loadRecords(false);
    } catch (err) {
      usageError = errorText('budget', err, t);
      // A new attempt after a failure is a new request, not a replay.
      usageKey = newClientId();
    } finally {
      usageBusy = false;
    }
  }
</script>

<div class="budget" data-testid="budget-card">
  {#if loading}
    <p class="muted">{t('common.loading')}</p>
  {/if}
  {#if error}<ErrorBanner message={error} />{/if}

  {#if summary}
    <ul class="remind">
      <li>{t('collab_relations.budget.note_units')}</li>
      <li>{t('collab_relations.budget.note_unknown')}</li>
      <li>{t('collab_relations.budget.note_enforcement')}</li>
    </ul>

    {#if summary.budgets.length === 0}
      <p class="muted" data-testid="budget-none">{t('collab_relations.budget.none')}</p>
    {/if}
    <ul class="list">
      {#each summary.budgets as b (`${b.unit}\u0000${b.source}`)}
        <li data-testid="budget-row">
          <div class="row">
            <strong class="grow text">{b.unit} · {b.source}</strong>
            <Button size="sm" variant="ghost" onclick={() => openSet(b)}>{t('collab_relations.budget.edit')}</Button>
          </div>
          <p class="muted text">
            {t('collab_relations.budget.amount')}: <strong>{b.amount} {b.unit}</strong>
            {#if b.alert_at}· {t('collab_relations.budget.alert_at')}: {b.alert_at} {b.unit}{/if}
            · {t('collab_relations.budget.sampling_period')}: {b.sampling_period}
            · {t('collab_relations.budget.version')} {b.version}
          </p>
          <p class="muted text">
            {t('collab_relations.budget.set_by', { kind: b.set_by_kind, id: b.set_by_id, time: formatTime(b.updated_at) })}
          </p>

          <div class="usage" data-testid="budget-usage">
            <span class="label">{t('collab_relations.budget.usage')}:</span>
            {#if b.usage.status === 'unknown' || b.usage.total === null}
              <span class="chip unknown">{t('collab_relations.budget.usage_unknown')}</span>
              <span class="muted">{t('collab_relations.budget.usage_unknown_hint')}</span>
            {:else}
              <strong class="text">{t('collab_relations.budget.usage_total', { total: b.usage.total, unit: b.unit })}</strong>
              <span class="muted">
                {t('collab_relations.budget.usage_records', { count: b.usage.record_count })}
                {#if b.usage.covered_from && b.usage.covered_to}
                  · {t('collab_relations.budget.usage_covered', {
                    from: formatTime(b.usage.covered_from),
                    to: formatTime(b.usage.covered_to),
                  })}
                {/if}
                {#if b.usage.last_reported_at}
                  · {t('collab_relations.budget.usage_last', { time: formatTime(b.usage.last_reported_at) })}
                {/if}
              </span>
            {/if}
          </div>
          {#if b.usage.overlapping_periods}
            <p class="warn">{t('collab_relations.budget.usage_overlap')}</p>
          {/if}
          <p class="muted" data-testid="budget-threshold">
            {#if b.over_threshold === null || !b.threshold}
              {t('collab_relations.budget.not_judged')}
            {:else}
              <span class={b.over_threshold ? 'warn' : ''}>
                {b.over_threshold ? t('collab_relations.budget.over') : t('collab_relations.budget.under')}
              </span>
              ·
              {t('collab_relations.budget.judged_basis', {
                basis:
                  b.threshold.basis === 'alert_at' || b.threshold.basis === 'amount'
                    ? t(`collab_relations.budget.basis.${b.threshold.basis}`)
                    : b.threshold.basis,
                threshold: b.threshold.threshold_amount,
                unit: b.threshold.unit,
                used: b.threshold.used_total,
                source: b.threshold.source,
              })}
              {#if b.threshold.total_may_double_count}· {t('collab_relations.budget.usage_overlap')}{/if}
            {/if}
          </p>
        </li>
      {/each}
    </ul>

    {#if summary.unmatched_usage.length > 0}
      <div class="unmatched" data-testid="budget-unmatched">
        <h3>{t('collab_relations.budget.unmatched_title')}</h3>
        <p class="muted">{t('collab_relations.budget.unmatched_note')}</p>
        <ul class="list">
          {#each summary.unmatched_usage as u (`${u.unit}\u0000${u.source}`)}
            <li class="text">
              {t('collab_relations.budget.unmatched_row', { unit: u.unit, source: u.source, total: u.total, count: u.record_count })}
              {#if u.overlapping_periods}<span class="warn"> · {t('collab_relations.budget.usage_overlap')}</span>{/if}
            </li>
          {/each}
        </ul>
      </div>
    {/if}

    <div class="records">
      <Button size="sm" variant="ghost" onclick={toggleRecords}>
        {showRecords ? t('collab_relations.budget.records_hide') : t('collab_relations.budget.records_show')}
      </Button>
      {#if showRecords}
        {#if recordsError}<ErrorBanner message={recordsError} />{/if}
        {#if !recordsLoading && !recordsError && records.length === 0}
          <p class="muted">{t('collab_relations.budget.records_none')}</p>
        {/if}
        <ul class="list">
          {#each records as r (r.id)}
            <li data-testid="usage-record">
              <p class="text">{t('collab_relations.budget.record_row', { amount: r.amount, unit: r.unit, source: r.source })}</p>
              <p class="muted">
                {t('collab_relations.budget.record_period', { from: formatTime(r.period_start), to: formatTime(r.period_end) })}
                · {t('collab_relations.budget.record_by', { kind: r.reported_by_kind, id: r.reported_by_id, time: formatTime(r.created_at) })}
              </p>
            </li>
          {/each}
        </ul>
        {#if recordsCursor}
          <Button size="sm" disabled={recordsLoading} onclick={() => loadRecords(true)}>{t('collab.load_more')}</Button>
        {/if}
      {/if}
    </div>

    <div class="writes">
      {#if !setOpen}<Button size="sm" onclick={() => openSet()}>{t('collab_relations.budget.set_open')}</Button>{/if}
      {#if !usageOpen}<Button size="sm" onclick={openUsage}>{t('collab_relations.budget.usage_open')}</Button>{/if}
    </div>

    {#if setOpen}
      <form class="form" onsubmit={submitSet} data-testid="budget-set-form">
        <h3>{t('collab_relations.budget.set_title')}</h3>
        <p class="form-hint">{t('collab_relations.budget.set_hint')}</p>
        <label class="field">
          <span class="form-label">{t('collab_relations.budget.unit')}</span>
          <input type="text" bind:value={sUnit} maxlength="32" autocomplete="off" />
        </label>
        {#if setProblems.unit}<p class="warn">{t('collab_relations.budget.bad_token', { max: 32 })}</p>{/if}
        <label class="field">
          <span class="form-label">{t('collab_relations.budget.source')}</span>
          <input type="text" bind:value={sSource} maxlength="64" autocomplete="off" />
        </label>
        {#if setProblems.source}<p class="warn">{t('collab_relations.budget.bad_token', { max: 64 })}</p>{/if}
        {#if sUnit && sSource}
          <p class="muted">
            {matching
              ? t('collab_relations.budget.set_replaces', { version: matching.version })
              : t('collab_relations.budget.set_creates')}
          </p>
        {/if}
        <label class="field">
          <span class="form-label">{t('collab_relations.budget.amount')}</span>
          <input type="text" inputmode="decimal" bind:value={sAmount} autocomplete="off" />
        </label>
        {#if setProblems.amount}<p class="warn">{t('collab_relations.budget.bad_amount')}</p>{/if}
        <label class="field">
          <span class="form-label">{t('collab_relations.budget.alert_at')}</span>
          <input type="text" inputmode="decimal" bind:value={sAlert} autocomplete="off" />
        </label>
        <p class="form-hint">{t('collab_relations.budget.alert_hint')}</p>
        {#if setProblems.alert}<p class="warn">{t('collab_relations.budget.bad_amount')}</p>{/if}
        <label class="field">
          <span class="form-label">{t('collab_relations.budget.sampling_period')}</span>
          <input type="text" bind:value={sPeriod} maxlength="64" autocomplete="off" />
        </label>
        <p class="form-hint">{t('collab_relations.budget.sampling_hint')}</p>
        {#if setError}<ErrorBanner message={setError} />{/if}
        <div class="form-actions">
          <Button type="submit" variant="primary" size="sm" disabled={setBusy || !setReady}>
            {setBusy ? t('common.saving') : t('collab_relations.budget.set_submit')}
          </Button>
          <Button size="sm" disabled={setBusy} onclick={() => (setOpen = false)}>{t('common.cancel')}</Button>
        </div>
      </form>
    {/if}

    {#if usageOpen}
      <form class="form" onsubmit={submitUsage} data-testid="usage-form">
        <h3>{t('collab_relations.budget.usage_title')}</h3>
        <p class="form-hint">{t('collab_relations.budget.usage_hint')}</p>
        <label class="field">
          <span class="form-label">{t('collab_relations.budget.unit')}</span>
          <input type="text" bind:value={uUnit} maxlength="32" autocomplete="off" />
        </label>
        {#if usageProblems.unit}<p class="warn">{t('collab_relations.budget.bad_token', { max: 32 })}</p>{/if}
        <label class="field">
          <span class="form-label">{t('collab_relations.budget.source')}</span>
          <input type="text" bind:value={uSource} maxlength="64" autocomplete="off" />
        </label>
        {#if usageProblems.source}<p class="warn">{t('collab_relations.budget.bad_token', { max: 64 })}</p>{/if}
        <label class="field">
          <span class="form-label">{t('collab_relations.budget.usage_amount')}</span>
          <input type="text" inputmode="decimal" bind:value={uAmount} autocomplete="off" />
        </label>
        {#if usageProblems.amount}<p class="warn">{t('collab_relations.budget.bad_amount')}</p>{/if}
        <label class="field">
          <span class="form-label">{t('collab_relations.budget.period_start')}</span>
          <input type="datetime-local" bind:value={uStart} />
        </label>
        <label class="field">
          <span class="form-label">{t('collab_relations.budget.period_end')}</span>
          <input type="datetime-local" bind:value={uEnd} />
        </label>
        {#if usageError}<ErrorBanner message={usageError} />{/if}
        <div class="form-actions">
          <Button type="submit" variant="primary" size="sm" disabled={usageBusy || !usageReady}>
            {usageBusy ? t('common.saving') : t('collab_relations.budget.usage_submit')}
          </Button>
          <Button size="sm" disabled={usageBusy} onclick={() => (usageOpen = false)}>{t('common.cancel')}</Button>
        </div>
      </form>
    {/if}
  {/if}
</div>

<style>
  .budget { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .remind { margin: 0; padding-left: 18px; color: var(--text-muted); font-size: 12px; }
  .list { list-style: none; margin: 0; padding: 0; }
  .list > li { padding: 8px 0; border-top: 1px solid var(--border-light); }
  .list > li:first-child { border-top: none; }
  .row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .grow { flex: 1 1 200px; min-width: 0; }
  .text { overflow-wrap: anywhere; }
  .muted { color: var(--text-muted); font-size: 12px; margin: 2px 0; overflow-wrap: anywhere; }
  .warn { color: var(--yellow); font-size: 12px; margin: 2px 0; }
  .label { font-size: 12px; color: var(--text-secondary); }
  .usage { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; margin-top: 4px; }
  .chip.unknown {
    display: inline-block; padding: 1px 8px; border-radius: 999px; font-size: 12px; font-weight: 600;
    border: 1px solid var(--border); background: var(--bg-tertiary); color: var(--text-secondary);
  }
  h3 { margin: 0; font-size: 14px; }
  .unmatched { border-top: 1px solid var(--border); padding-top: 8px; }
  .writes { display: flex; gap: 8px; flex-wrap: wrap; }
  .form { display: flex; flex-direction: column; gap: 8px; border-top: 1px solid var(--border); padding-top: 10px; }
  .field { display: flex; flex-direction: column; gap: 4px; }
  input { width: 100%; box-sizing: border-box; }
</style>
