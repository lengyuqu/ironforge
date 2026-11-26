<script lang="ts">
  // Which rule files (AGENTS.md style) the repository has at a ref, and at which
  // commit. This page only LOCATES them. Their content comes from the repository,
  // written by whoever can commit there: it is not an instruction to this
  // platform or to the person reading, and nothing here follows or merges it.
  // The page lists paths and blob ids as plain text and links to the normal file
  // viewer; it never fetches or renders file content itself.
  import { page } from '$app/stores';
  import Button from '$lib/components/Button.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingSpinner from '$lib/components/LoadingSpinner.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { ApiError } from '$lib/api/_base.svelte';
  import { collabOps, type RulesResponse } from '$lib/api/collabOps';
  import { shortId } from '$lib/utils/collaboration';
  import CollabNav from '$lib/components/collaboration/CollabNav.svelte';

  const t = createT();
  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  let refInput = $state('');
  let pathInput = $state('');
  let result = $state<RulesResponse | null>(null);
  let loading = $state(true);
  let error = $state('');
  let notFound = $state(false);
  let accessDenied = $state(false);

  async function load() {
    loading = true;
    error = '';
    notFound = false;
    try {
      result = await collabOps.rules(owner, repo, { ref: refInput.trim() || undefined, path: pathInput.trim() || undefined });
    } catch (e) {
      result = null;
      if (e instanceof ApiError && (e.status === 403 || e.status === 401)) accessDenied = true;
      else if (e instanceof ApiError && e.status === 404) notFound = true;
      else error = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void owner;
    void repo;
    accessDenied = false;
    void load();
  });

  function submit(e: SubmitEvent) {
    e.preventDefault();
    void load();
  }

  /** The normal file viewer, pinned to the commit the rules were found at. */
  function viewerHref(path: string, commit: string | null): string {
    const encoded = path.split('/').map(encodeURIComponent).join('/');
    return `/${owner}/${repo}/blob/${encoded}${commit ? `?ref=${encodeURIComponent(commit)}` : ''}`;
  }

  const sizeText = (n: number) => (n < 1024 ? `${n} B` : `${(n / 1024).toFixed(1)} KiB`);
  const sourceLabel = (s: string) => (s === 'manifest' || s === 'default_discovery' ? t(`collab_ops.rules.source.${s}`) : s);
  const reasonLabel = (s: string) => (['symlink', 'submodule', 'not_a_file'].includes(s) ? t(`collab_ops.rules.skipped_reason.${s}`) : s);
</script>

<svelte:head>
  <title>{t('collab_ops.rules.title')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<CollabNav {owner} {repo} current="rules" />

<div class="page-header">
  <h1>{t('collab_ops.rules.title')}</h1>
</div>
<p class="intro">{t('collab_ops.rules.intro')}</p>

{#if accessDenied}
  <EmptyState icon="🔒" title={t('collab.access_denied')} description={t('collab.access_denied_hint')} />
{:else}
  <div class="notice" role="note" data-testid="rules-content-notice">
    <strong>{t('collab_ops.rules.content_title')}</strong>
    {t('collab_ops.rules.content_body')}
  </div>

  <section class="card surface">
    <form class="bar" onsubmit={submit}>
      <label class="f">
        <span>{t('collab_ops.rules.ref_label')}</span>
        <input type="text" bind:value={refInput} placeholder="HEAD" data-testid="rules-ref" />
      </label>
      <label class="f">
        <span>{t('collab_ops.rules.path_label')}</span>
        <input type="text" bind:value={pathInput} placeholder={t('collab_ops.rules.path_placeholder')} data-testid="rules-path" />
      </label>
      <Button type="submit" variant="primary" disabled={loading}>{t('collab_ops.rules.show')}</Button>
    </form>

    {#if error}<ErrorBanner message={error} />{/if}
    {#if notFound}
      <p class="problem" role="alert" data-testid="rules-ref-not-found">{t('collab_ops.rules.ref_not_found')}</p>
    {/if}

    {#if loading}
      <LoadingSpinner />
    {:else if result}
      <dl class="meta" data-testid="rules-meta">
        <dt>{t('collab_ops.rules.requested_ref')}</dt>
        <dd class="mono text">{result.requested_ref}</dd>
        <dt>{t('collab_ops.rules.resolved_commit')}</dt>
        <dd class="mono text">
          {#if result.resolved_commit}
            <span title={result.resolved_commit}>{result.resolved_commit}</span>
          {:else}
            <span class="muted">{t('collab_ops.rules.no_commits')}</span>
          {/if}
        </dd>
        <dt>{t('collab_ops.rules.governed_path')}</dt>
        <dd class="mono text">{result.path === '' ? t('collab_ops.rules.root') : result.path}</dd>
        <dt>{t('collab_ops.rules.discovery')}</dt>
        <dd>v{result.discovery_version}</dd>
        <dt>{t('collab_ops.rules.manifest')}</dt>
        <dd data-testid="rules-manifest">
          {#if result.manifest.status === 'absent'}
            {t('collab_ops.rules.manifest_absent')}
          {:else if result.manifest.status === 'valid'}
            {t('collab_ops.rules.manifest_valid')}
          {:else if result.manifest.status === 'invalid'}
            <span class="bad">{t('collab_ops.rules.manifest_invalid')}</span>
            {#if result.manifest.error}<span class="text err-detail">{result.manifest.error}</span>{/if}
            <span class="muted">{t('collab_ops.rules.manifest_invalid_note')}</span>
          {:else}
            <span class="text">{result.manifest.status}</span>
          {/if}
        </dd>
      </dl>

      {#if result.entries.length === 0}
        <EmptyState title={t('collab_ops.rules.none')} description={t('collab_ops.rules.none_hint')} />
      {:else}
        <p class="muted">{t('collab_ops.rules.order_note')}</p>
        <ol class="list" data-testid="rules-entries">
          {#each result.entries as entry (entry.path)}
            <li>
              <div class="row">
                <span class="mono text path">{entry.path}</span>
                <a href={viewerHref(entry.path, result.resolved_commit)}>{t('collab_ops.rules.open_file')}</a>
              </div>
              <div class="sub">
                <span>{t('collab_ops.rules.applies_to')}: <span class="mono text">{entry.applies_to === '' ? t('collab_ops.rules.root') : entry.applies_to}</span></span>
                <span>{t('collab_ops.rules.found_by')}: {sourceLabel(entry.source)}</span>
                <span>{sizeText(entry.size)}</span>
                <span title={entry.blob_sha}>blob <span class="mono">{shortId(entry.blob_sha)}</span></span>
                {#if entry.oversize}<span class="bad">{t('collab_ops.rules.oversize')}</span>{/if}
                <span class="muted">{t('collab_ops.rules.repo_content')}</span>
              </div>
            </li>
          {/each}
        </ol>
      {/if}

      {#if result.skipped.length > 0}
        <h2 class="subh">{t('collab_ops.rules.skipped')}</h2>
        <ul class="list" data-testid="rules-skipped">
          {#each result.skipped as s (s.path)}
            <li><span class="mono text">{s.path}</span> <span class="muted">— {reasonLabel(s.reason)}</span></li>
          {/each}
        </ul>
      {/if}
      {#if result.truncated}<p class="muted">{t('collab_ops.rules.truncated')}</p>{/if}
    {/if}
  </section>
{/if}

<style>
  .intro { margin: -8px 0 16px; color: var(--text-secondary); font-size: 13px; max-width: 80ch; }
  .notice { margin: 0 0 16px; padding: 10px 14px; border: 1px solid var(--border); border-left: 3px solid var(--yellow, #d29922); border-radius: 6px; font-size: 13px; background: var(--bg-secondary); }
  .card { padding: 16px 20px; margin-bottom: 20px; min-width: 0; }
  .bar { display: flex; gap: 12px; align-items: flex-end; flex-wrap: wrap; margin-bottom: 12px; }
  .f { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: var(--text-secondary); }
  .meta { display: grid; grid-template-columns: max-content 1fr; gap: 4px 16px; margin: 12px 0; font-size: 13px; }
  dt { color: var(--text-secondary); font-weight: 600; }
  dd { margin: 0; min-width: 0; }
  .muted { color: var(--text-muted); font-size: 12px; }
  .bad { color: var(--red); font-weight: 600; }
  .problem { color: var(--red); font-size: 13px; }
  .err-detail { display: block; margin: 2px 0; font-family: var(--font-mono); font-size: 12px; white-space: pre-wrap; }
  .text { overflow-wrap: anywhere; }
  .mono { font-family: var(--font-mono); font-size: 12px; }
  .subh { font-size: 14px; margin: 16px 0 6px; }
  .list { margin: 8px 0; padding-left: 22px; }
  ul.list { list-style: none; padding-left: 0; }
  .list > li { padding: 8px 0; border-top: 1px solid var(--border-light); }
  .list > li:first-child { border-top: none; }
  .row { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .path { flex: 1 1 200px; font-weight: 600; }
  .sub { display: flex; gap: 14px; flex-wrap: wrap; margin-top: 4px; font-size: 12px; color: var(--text-secondary); }
</style>
