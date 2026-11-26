<script lang="ts">
  // One coordination group: its text, and the links the signed-in person is
  // allowed to see.
  //
  // Privacy rules this page keeps:
  //  * Links to repositories the reader cannot see are simply absent. The page
  //    never shows a total or a count of links, never says anything is hidden,
  //    and an empty list reads the same whether the group has no links or none
  //    visible to this reader.
  //  * Each page of links is authorised again on the server, so a page may hold
  //    fewer items than requested while more follow (`next_cursor`).
  //  * `version` only exists for people who can write to the anchor repository;
  //    readers get no write controls.
  import { page } from '$app/stores';
  import CollabNav from '$lib/components/collaboration/CollabNav.svelte';
  import SectionCard from '$lib/components/collaboration/SectionCard.svelte';
  import StatusBadge from '$lib/components/collaboration/StatusBadge.svelte';
  import GroupAddLinkForm from '$lib/components/collaboration/GroupAddLinkForm.svelte';
  import Button from '$lib/components/Button.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingSpinner from '$lib/components/LoadingSpinner.svelte';
  import { confirmDialog } from '$lib/components/confirm.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { ApiError } from '$lib/api/_base.svelte';
  import { collabGroups, type GroupLink, type GroupView } from '$lib/api/collabGroups';
  import { KNOWN, formatTime, newClientId, shortId } from '$lib/utils/collaboration';
  import { forget, remember } from '$lib/utils/collabGroups';

  const t = createT();
  const msg = (e: unknown) => toErrorMessage(e, t('common.unknown_error'));

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let groupId = $derived($page.params.id!);

  let group = $state<GroupView | null>(null);
  let links = $state<GroupLink[]>([]);
  let nextCursor = $state<string | null>(null);
  let asOf = $state('');
  let loading = $state(true);
  let moreLoading = $state(false);
  let error = $state('');
  let notice = $state('');
  let denied = $state(false);
  let notFound = $state(false);
  let removing = $state<string | null>(null);

  // Present only for people who can write to the anchor repository.
  let version = $derived(group?.version);

  async function load() {
    loading = true;
    error = '';
    denied = false;
    notFound = false;
    try {
      const res = await collabGroups.get(owner, repo, groupId, { limit: 50 });
      const { links: first, ...rest } = res;
      group = rest;
      links = first.items;
      nextCursor = first.next_cursor;
      asOf = first.as_of;
      remember(owner, repo, { id: rest.id, title: rest.title });
    } catch (e) {
      group = null;
      links = [];
      nextCursor = null;
      if (e instanceof ApiError && (e.status === 403 || e.status === 401)) denied = true;
      else if (e instanceof ApiError && e.status === 404) {
        notFound = true;
        forget(owner, repo, groupId);
      } else error = msg(e);
    } finally {
      loading = false;
    }
  }

  async function loadMore() {
    if (!nextCursor || moreLoading) return;
    moreLoading = true;
    error = '';
    try {
      const res = await collabGroups.listLinks(owner, repo, groupId, { limit: 50, cursor: nextCursor });
      // Keep everything already shown; a link can only appear once.
      const seen = new Set(links.map((l) => l.id));
      links = [...links, ...res.items.filter((l) => !seen.has(l.id))];
      nextCursor = res.next_cursor;
      asOf = res.as_of;
    } catch (e) {
      error = msg(e);
    } finally {
      moreLoading = false;
    }
  }

  $effect(() => {
    void owner;
    void repo;
    void groupId;
    notice = '';
    void load();
  });

  let removeKeys = new Map<string, string>();
  function removeKey(linkId: string) {
    let k = removeKeys.get(linkId);
    if (!k) {
      k = newClientId();
      removeKeys.set(linkId, k);
    }
    return k;
  }

  async function removeLink(link: GroupLink) {
    if (version === undefined || removing) return;
    const ok = await confirmDialog({
      title: t('collab_groups.links.remove_confirm_title'),
      message: t('collab_groups.links.remove_confirm_message'),
      confirmLabel: t('collab_groups.links.remove'),
      variant: 'danger',
    });
    if (!ok) return;
    removing = link.id;
    error = '';
    notice = '';
    try {
      await collabGroups.removeLink(owner, repo, groupId, link.id, {
        client_request_id: removeKey(link.id),
        expected_group_version: version,
      });
      removeKeys.delete(link.id);
      toast.success(t('collab_groups.links.removed_toast'));
      await load();
    } catch (e) {
      // A new attempt after a failure is a new request, not a replay.
      removeKeys.delete(link.id);
      if (e instanceof ApiError && (e.code === 'VERSION_CONFLICT' || e.code === 'LINK_ALREADY_REMOVED')) {
        notice = t('collab_groups.conflict');
        await load();
      } else {
        error = msg(e);
      }
    } finally {
      removing = null;
    }
  }

  const repoHref = (l: GroupLink) => `/${l.target_repo.owner}/${l.target_repo.name}`;
  const taskHref = (l: GroupLink) => `/${l.target_repo.owner}/${l.target_repo.name}/collaboration/tasks/${l.task!.id}`;
</script>

<svelte:head>
  <title>{group?.title ?? t('collab_groups.detail.title')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<CollabNav {owner} {repo} current="groups" />

<p class="back"><a href={`/${owner}/${repo}/collaboration/groups`}>← {t('collab_groups.detail.back')}</a></p>

{#if loading && !group}
  <LoadingSpinner />
{:else if denied}
  <EmptyState icon="🔒" title={t('collab.access_denied')} description={t('collab.access_denied_hint')} />
{:else if notFound}
  <EmptyState title={t('collab_groups.detail.not_found')} description={t('collab_groups.detail.not_found_hint')} />
{:else if group}
  <div class="page-header"><h1 class="text" data-testid="group-title">{group.title}</h1></div>
  <p class="muted">
    {t('collab_groups.detail.created', {
      who: `${group.created_by_kind} ${shortId(group.created_by_id)}`,
      time: formatTime(group.created_at),
    })}
    · {t('collab_groups.detail.declared')}
  </p>
  {#if group.description}
    <p class="desc text" data-testid="group-description">{group.description}</p>
  {:else}
    <p class="muted">{t('collab_groups.detail.no_description')}</p>
  {/if}
  <p class="warn">{t('collab_groups.sharing_warning')}</p>
  {#if version !== undefined}
    <p class="muted" data-testid="group-version">{t('collab_groups.detail.version', { version })}</p>
  {:else}
    <p class="muted" data-testid="read-only-note">{t('collab_groups.detail.read_only')}</p>
  {/if}

  {#if notice}<p class="notice" role="status" data-testid="conflict-notice">{notice}</p>{/if}
  {#if error}<ErrorBanner message={error} />{/if}

  <SectionCard title={t('collab_groups.links.title')} note={t('collab_groups.links.note')}>
    {#if links.length === 0}
      <p class="muted" data-testid="links-empty">{t('collab_groups.links.empty')}</p>
    {:else}
      <ul class="list" data-testid="links-list">
        {#each links as link (link.id)}
          <li data-testid="group-link">
            <div class="row">
              <a class="text mono" href={repoHref(link)}>{link.target_repo.owner}/{link.target_repo.name}</a>
              {#if version !== undefined}
                <Button
                  size="sm"
                  variant="danger"
                  disabled={removing !== null}
                  onclick={() => removeLink(link)}
                >{t('collab_groups.links.remove')}</Button>
              {/if}
            </div>
            {#if link.task}
              <div class="row sub">
                <span class="muted">{t('collab_groups.links.task')}</span>
                <a class="text" href={taskHref(link)}>{link.task.title}</a>
                <StatusBadge
                  value={link.task.status}
                  prefix="collab.task_status"
                  known={KNOWN.taskStatus}
                  good={['done']}
                  warn={['waiting', 'blocked']}
                  title={t('collab.declared_hint')}
                />
              </div>
            {/if}
            {#if link.commit}
              <div class="row sub">
                <span class="muted">{t('collab_groups.links.commit')}</span>
                <code class="mono">{shortId(link.commit.sha)}</code>
                <StatusBadge
                  value={link.commit.commit_state}
                  prefix="collab_groups.commit_state"
                  known={['available', 'unavailable']}
                  good={['available']}
                  warn={['unavailable']}
                />
              </div>
            {/if}
            <p class="muted">
              {t('collab_groups.links.added', {
                who: `${link.created_by_kind} ${shortId(link.created_by_id)}`,
                time: formatTime(link.created_at),
              })}
            </p>
          </li>
        {/each}
      </ul>
    {/if}
    {#if nextCursor}
      <div class="more">
        <Button disabled={moreLoading} onclick={loadMore}>
          {moreLoading ? t('common.loading') : t('collab_groups.links.load_more')}
        </Button>
      </div>
    {/if}
    {#if asOf}<p class="muted" data-testid="links-as-of">{t('collab_groups.links.as_of', { time: formatTime(asOf) })}</p>{/if}
  </SectionCard>

  {#if version !== undefined}
    <SectionCard title={t('collab_groups.add.title')} note={t('collab_groups.add.note')}>
      <GroupAddLinkForm {owner} {repo} {groupId} {version} onChanged={load} />
    </SectionCard>
  {/if}
{:else if error}
  <ErrorBanner message={error} />
{/if}

<style>
  .back { margin: 0 0 8px; font-size: 13px; }
  .muted { color: var(--text-muted); font-size: 12px; margin: 4px 0; }
  .text { overflow-wrap: anywhere; min-width: 0; }
  .desc { white-space: pre-wrap; margin: 8px 0; max-width: 80ch; }
  .warn { margin: 8px 0; padding: 8px 10px; font-size: 12px; background: var(--yellow-subtle); border: 1px solid var(--yellow-dim); border-radius: 6px; color: var(--text-secondary); max-width: 80ch; }
  .notice { margin: 8px 0; padding: 8px 10px; font-size: 13px; border: 1px solid var(--border); border-radius: 6px; background: var(--bg-tertiary); }
  .list { list-style: none; margin: 0; padding: 0; }
  .list > li { padding: 10px 0; border-top: 1px solid var(--border-light); }
  .list > li:first-child { border-top: none; }
  .row { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; justify-content: space-between; }
  .row.sub { justify-content: flex-start; margin-top: 4px; }
  .mono { font-family: var(--font-mono); font-size: 12px; }
  .more { margin-top: 12px; text-align: center; }
</style>
