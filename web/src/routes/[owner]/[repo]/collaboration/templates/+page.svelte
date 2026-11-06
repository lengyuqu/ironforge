<script lang="ts">
  // Task templates. Repository admins maintain them; every version is immutable
  // (an edit is a new version) and a task made from a template stays pinned to
  // the version it was made from. Template text is plain text, never markup.
  //
  // Creating a task FROM a template is not offered here: the server only accepts
  // that call from an agent session (it needs the session secret), and a signed-in
  // person has none. The page says so rather than working around it.
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import Button from '$lib/components/Button.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { ApiError } from '$lib/api/_base.svelte';
  import { collabOps, type TemplateView } from '$lib/api/collabOps';
  import { formatTime } from '$lib/utils/collaboration';
  import SectionCard from '$lib/components/collaboration/SectionCard.svelte';
  import CollabNav from '$lib/components/collaboration/CollabNav.svelte';
  import TemplateForm from '$lib/components/collaboration/TemplateForm.svelte';
  import TemplateContent from '$lib/components/collaboration/TemplateContent.svelte';

  const t = createT();
  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  /** Versions fetched per template when its history is opened (newest first). */
  const HISTORY_LIMIT = 50;

  let templates = $state<TemplateView[]>([]);
  let nextCursor = $state<string | null>(null);
  let loading = $state(true);
  let moreLoading = $state(false);
  let error = $state('');
  let accessDenied = $state(false);

  let creating = $state(false);
  let editingId = $state<string | null>(null);
  let openId = $state<string | null>(null);
  // Bumped when a form must be rebuilt from fresh server data (after a conflict).
  let formEpoch = $state(0);

  type History = { versions: TemplateView[]; selected: number; error: string; loading: boolean };
  let histories = $state<Record<string, History>>({});

  async function load(append = false) {
    if (append) moreLoading = true;
    else loading = true;
    error = '';
    try {
      const res = await collabOps.listTemplates(owner, repo, { limit: 25, cursor: append ? nextCursor : null });
      templates = append ? [...templates, ...res.items] : res.items;
      nextCursor = res.next_cursor;
    } catch (e) {
      if (e instanceof ApiError && (e.status === 403 || e.status === 401)) accessDenied = true;
      else error = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      loading = false;
      moreLoading = false;
    }
  }

  $effect(() => {
    void owner;
    void repo;
    accessDenied = false;
    creating = false;
    editingId = null;
    openId = null;
    histories = {};
    load(false);
  });

  async function loadHistory(tpl: TemplateView) {
    const first = Math.max(1, tpl.latest_version - HISTORY_LIMIT + 1);
    histories[tpl.id] = { versions: [], selected: tpl.latest_version, error: '', loading: true };
    try {
      const nums: number[] = [];
      for (let v = tpl.latest_version; v >= first; v--) nums.push(v);
      const versions = await Promise.all(nums.map((v) => (v === tpl.version.version ? Promise.resolve(tpl) : collabOps.getTemplate(owner, repo, tpl.id, v))));
      histories[tpl.id] = { versions, selected: tpl.latest_version, error: '', loading: false };
    } catch (e) {
      histories[tpl.id] = { versions: [], selected: tpl.latest_version, error: toErrorMessage(e, t('common.unknown_error')), loading: false };
    }
  }

  function toggle(tpl: TemplateView) {
    if (openId === tpl.id) {
      openId = null;
      return;
    }
    openId = tpl.id;
    void loadHistory(tpl);
  }

  function startEdit(tpl: TemplateView) {
    creating = false;
    editingId = tpl.id;
    formEpoch += 1;
  }

  async function afterSaved(saved: TemplateView) {
    creating = false;
    editingId = null;
    await load(false);
    if (openId === saved.id) {
      const fresh = templates.find((x) => x.id === saved.id);
      if (fresh) void loadHistory(fresh);
    }
  }

  /** The template changed since the editor opened: re-read it and rebuild the form from the new latest version. */
  async function afterConflict(id: string) {
    await load(false);
    formEpoch += 1;
    if (!templates.some((x) => x.id === id)) editingId = null;
  }

  let adminHint = $derived(t('collab_ops.templates.admin_only'));
</script>

<svelte:head>
  <title>{t('collab_ops.templates.title')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="collaboration" />
  <CollabNav {owner} {repo} current="templates" />

  <div class="page-header">
    <h1>{t('collab_ops.templates.title')}</h1>
  </div>
  <p class="intro">{t('collab_ops.templates.intro')}</p>

  {#if accessDenied}
    <EmptyState icon="🔒" title={t('collab.access_denied')} description={t('collab.access_denied_hint')} />
  {:else}
    <div class="notice" role="note" data-testid="from-template-notice">
      <strong>{t('collab_ops.templates.no_task_title')}</strong>
      {t('collab_ops.templates.no_task_body')}
    </div>

    <SectionCard
      title={t('collab_ops.templates.list_title')}
      note={adminHint}
      {loading}
      {error}
      count={loading ? null : templates.length}
    >
      {#snippet actions()}
        <Button
          variant="primary"
          size="sm"
          onclick={() => {
            creating = !creating;
            editingId = null;
          }}
        >{t('collab_ops.templates.new')}</Button>
      {/snippet}

      {#if creating}
        {#key formEpoch}
          <TemplateForm {owner} {repo} template={null} onSaved={afterSaved} onCancel={() => (creating = false)} />
        {/key}
      {/if}

      {#if templates.length === 0}
        <EmptyState title={t('collab_ops.templates.empty')} description={t('collab_ops.templates.empty_hint')} />
      {:else}
        <ul class="list">
          {#each templates as tpl (tpl.id)}
            <li data-testid="template-row">
              <div class="row">
                <span class="grow text name">{tpl.name}</span>
                <span class="ver" title={t('collab_ops.templates.latest_hint')}>v{tpl.latest_version}</span>
                <span class="when">{formatTime(tpl.updated_at)}</span>
                <Button size="sm" onclick={() => toggle(tpl)}>
                  {openId === tpl.id ? t('collab.hide') : t('collab_ops.templates.details')}
                </Button>
                <Button size="sm" onclick={() => startEdit(tpl)}>{t('collab_ops.templates.edit')}</Button>
              </div>

              {#if editingId === tpl.id}
                {#key formEpoch}
                  <TemplateForm
                    {owner}
                    {repo}
                    template={tpl}
                    onSaved={afterSaved}
                    onCancel={() => (editingId = null)}
                    onConflict={() => afterConflict(tpl.id)}
                  />
                {/key}
              {/if}

              {#if openId === tpl.id}
                {@const h = histories[tpl.id]}
                <div class="detail">
                  {#if !h || h.loading}
                    <p class="muted">{t('common.loading')}</p>
                  {:else if h.error}
                    <p class="err" role="alert">{h.error}</p>
                  {:else}
                    <div class="versions" role="tablist" aria-label={t('collab_ops.templates.versions')}>
                      {#each h.versions as v (v.version.version)}
                        <button
                          type="button"
                          role="tab"
                          aria-selected={h.selected === v.version.version}
                          class:sel={h.selected === v.version.version}
                          onclick={() => (histories[tpl.id].selected = v.version.version)}
                        >v{v.version.version}</button>
                      {/each}
                    </div>
                    {#each h.versions.filter((v) => v.version.version === h.selected) as v (v.version.version)}
                      <p class="muted">
                        {t('collab_ops.templates.version_meta', {
                          version: v.version.version,
                          user: v.version.created_by_user_id,
                          time: formatTime(v.version.created_at),
                        })}
                        {#if v.version.version !== tpl.latest_version}· {t('collab_ops.templates.older_version')}{/if}
                      </p>
                      <TemplateContent content={v.version} />
                    {/each}
                    {#if tpl.latest_version > HISTORY_LIMIT}
                      <p class="muted">{t('collab_ops.templates.history_capped', { n: HISTORY_LIMIT })}</p>
                    {/if}
                    <p class="muted">{t('collab_ops.templates.immutable_note')}</p>
                  {/if}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
        {#if nextCursor}
          <div class="more">
            <Button disabled={moreLoading} onclick={() => load(true)}>
              {moreLoading ? t('common.loading') : t('collab.load_more')}
            </Button>
          </div>
        {/if}
      {/if}
    </SectionCard>
  {/if}
</div>

<style>
  .intro { margin: -8px 0 16px; color: var(--text-secondary); font-size: 13px; max-width: 80ch; }
  .notice { margin: 0 0 16px; padding: 10px 14px; border: 1px solid var(--border); border-left: 3px solid var(--yellow, #d29922); border-radius: 6px; font-size: 13px; background: var(--bg-secondary); }
  .muted { color: var(--text-muted); font-size: 12px; margin: 4px 0; }
  .err { color: var(--red); font-size: 13px; }
  .list { list-style: none; margin: 0; padding: 0; }
  .list > li { padding: 10px 0; border-top: 1px solid var(--border-light); }
  .list > li:first-child { border-top: none; }
  .row { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .grow { flex: 1 1 200px; min-width: 0; }
  .text { overflow-wrap: anywhere; }
  .name { font-weight: 600; }
  .ver { padding: 0 8px; border-radius: 999px; font-size: 12px; background: var(--bg-tertiary); color: var(--text-secondary); font-weight: 600; }
  .when { color: var(--text-muted); font-size: 12px; white-space: nowrap; }
  .detail { margin: 8px 0 0 8px; padding-left: 12px; border-left: 2px solid var(--border-light); }
  .versions { display: flex; gap: 4px; flex-wrap: wrap; }
  .versions button { padding: 2px 10px; border: 1px solid var(--border); border-radius: 999px; background: transparent; color: var(--text-secondary); font-size: 12px; cursor: pointer; }
  .versions button.sel { background: var(--bg-tertiary); color: var(--text-primary); font-weight: 600; }
  .more { margin-top: 12px; text-align: center; }
</style>
