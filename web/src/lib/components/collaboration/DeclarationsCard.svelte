<script lang="ts">
  // What agent sessions SAY about themselves: tools, whether they can run tests,
  // their network situation. These are the sessions' own claims. The server marks
  // every one `source: user_content` and `authorization: none`: they can be wrong
  // or out of date, and they never grant or deny a permission. A session that
  // never declared is absent from the list, which means unknown, not "cannot".
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import Button from '$lib/components/Button.svelte';
  import SectionCard from './SectionCard.svelte';
  import StatusBadge from './StatusBadge.svelte';
  import { collabOps, type Declaration } from '$lib/api/collabOps';
  import { loadSessionMap, type CollabSession } from '$lib/api/collaboration';
  import { KNOWN, formatTime, shortId } from '$lib/utils/collaboration';

  interface Props {
    owner: string;
    repo: string;
  }
  let { owner, repo }: Props = $props();
  const t = createT();

  const TESTS = ['yes', 'no', 'unknown'] as const;
  const NETWORK = ['unrestricted', 'restricted', 'offline', 'unknown'] as const;
  const STALE_REASONS = ['age', 'session_ended', 'session_expired'] as const;

  let items = $state<Declaration[]>([]);
  let nextCursor = $state<string | null>(null);
  let loading = $state(true);
  let moreLoading = $state(false);
  let error = $state('');
  let sessions = $state<Map<string, CollabSession> | null>(null);

  async function load(append = false) {
    if (append) moreLoading = true;
    else loading = true;
    error = '';
    try {
      const res = await collabOps.listDeclarations(owner, repo, { limit: 25, cursor: append ? nextCursor : null });
      items = append ? [...items, ...res.items] : res.items;
      nextCursor = res.next_cursor;
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      loading = false;
      moreLoading = false;
    }
  }

  $effect(() => {
    void owner;
    void repo;
    loadSessionMap(owner, repo)
      .then((m) => (sessions = m))
      .catch(() => (sessions = null));
    void load(false);
  });

  const who = (id: string) => sessions?.get(id)?.display_name || shortId(id);
  const choice = (prefix: string, v: string, known: readonly string[]) => (known.includes(v) ? t(`${prefix}.${v}`) : v);
  const staleReason = (r: string) => ((STALE_REASONS as readonly string[]).includes(r) ? t(`collab_ops.declarations.stale_reason.${r}`) : r);
</script>

<SectionCard
  title={t('collab_ops.declarations.title')}
  note={t('collab_ops.declarations.note')}
  {loading}
  {error}
  count={loading ? null : items.length}
>
  <p class="claims" role="note" data-testid="declarations-claims-note">{t('collab_ops.declarations.not_permissions')}</p>
  {#if items.length === 0}
    <p class="muted">{t('collab_ops.declarations.empty')}</p>
  {:else}
    <ul class="list" data-testid="declarations-list">
      {#each items as d (d.session_id)}
        <li>
          <div class="row">
            <span class="text name">{who(d.session_id)}</span>
            <StatusBadge value={d.session_state} prefix="collab.session_state" known={KNOWN.sessionState} good={['online']} />
            <span class="auth" title={t('collab_ops.declarations.auth_none_hint')}>{t('collab_ops.declarations.auth_none')}</span>
            {#if d.stale}
              <span class="stale" data-testid="declaration-stale">{t('collab_ops.declarations.stale')}
                ({d.stale_reasons.map(staleReason).join(', ')})</span>
            {/if}
            <span class="when">{t('collab_ops.declarations.declared_at', { time: formatTime(d.declared_at), version: d.version })}</span>
          </div>
          <dl class="kv">
            <dt>{t('collab_ops.declarations.claims_tests')}</dt>
            <dd>{choice('collab_ops.declarations.tests', d.can_run_tests, TESTS)}</dd>
            <dt>{t('collab_ops.declarations.claims_network')}</dt>
            <dd>
              {choice('collab_ops.declarations.network', d.network, NETWORK)}
              {#if d.network_notes}<span class="text note">— {d.network_notes}</span>{/if}
            </dd>
            <dt>{t('collab_ops.declarations.claims_tools')}</dt>
            <dd>
              {#if d.tools.length === 0}
                <span class="muted">{t('collab_ops.none')}</span>
              {:else}
                <span class="tools">{#each d.tools as tool}<span class="tool text">{tool}</span>{/each}</span>
              {/if}
            </dd>
            {#if d.notes}
              <dt>{t('collab_ops.declarations.notes')}</dt>
              <dd class="text note pre">{d.notes}</dd>
            {/if}
          </dl>
        </li>
      {/each}
    </ul>
    {#if nextCursor}
      <div class="more">
        <Button disabled={moreLoading} onclick={() => load(true)}>{moreLoading ? t('common.loading') : t('collab.load_more')}</Button>
      </div>
    {/if}
  {/if}
</SectionCard>

<style>
  .claims { margin: 0 0 10px; padding: 8px 12px; border-left: 3px solid var(--yellow, #d29922); background: var(--bg-secondary); font-size: 12px; }
  .muted { color: var(--text-muted); font-size: 13px; margin: 4px 0; }
  .list { list-style: none; margin: 0; padding: 0; }
  .list > li { padding: 10px 0; border-top: 1px solid var(--border-light); }
  .list > li:first-child { border-top: none; }
  .row { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .name { font-weight: 600; }
  .text { overflow-wrap: anywhere; min-width: 0; }
  .pre { white-space: pre-wrap; }
  .auth { font-size: 11px; padding: 0 6px; border: 1px dashed var(--border); border-radius: 4px; color: var(--text-muted); }
  .stale { font-size: 12px; color: var(--yellow, #d29922); font-weight: 600; }
  .when { margin-left: auto; font-size: 12px; color: var(--text-muted); }
  .kv { display: grid; grid-template-columns: fit-content(55%) minmax(0, 1fr); overflow-wrap: anywhere; gap: 2px 14px; margin: 6px 0 0; font-size: 13px; }
  dt { color: var(--text-secondary); }
  dd { margin: 0; min-width: 0; }
  .note { color: var(--text-secondary); }
  .tools { display: flex; flex-wrap: wrap; gap: 4px; }
  .tool { padding: 0 8px; border-radius: 4px; background: var(--bg-tertiary); font-family: var(--font-mono); font-size: 12px; }
  .more { margin-top: 12px; text-align: center; }
</style>
