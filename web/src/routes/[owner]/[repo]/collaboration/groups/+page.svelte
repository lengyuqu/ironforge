<script lang="ts">
  // Cross-repository coordination groups anchored at this repository.
  //
  // The server has no endpoint that LISTS a repository's groups: a group can be
  // created, and opened by its id. This page therefore offers create, open-by-id
  // and a browser-local "recently opened" list, and says so instead of showing
  // anything that looks like a complete list.
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import CollabNav from '$lib/components/collaboration/CollabNav.svelte';
  import SectionCard from '$lib/components/collaboration/SectionCard.svelte';
  import GroupCreateForm from '$lib/components/collaboration/GroupCreateForm.svelte';
  import Button from '$lib/components/Button.svelte';
  import { createT } from '$lib/i18n';
  import type { GroupView } from '$lib/api/collabGroups';
  import { forget, loadRemembered, remember, type RememberedGroup } from '$lib/utils/collabGroups';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  let remembered = $state<RememberedGroup[]>([]);
  let openId = $state('');

  $effect(() => {
    remembered = loadRemembered(owner, repo);
  });

  const groupHref = (id: string) => `/${owner}/${repo}/collaboration/groups/${encodeURIComponent(id)}`;

  function onCreated(group: GroupView) {
    remember(owner, repo, { id: group.id, title: group.title });
    void goto(groupHref(group.id));
  }

  function open(e: SubmitEvent) {
    e.preventDefault();
    const id = openId.trim();
    if (id) void goto(groupHref(id));
  }

  function drop(id: string) {
    forget(owner, repo, id);
    remembered = loadRemembered(owner, repo);
  }
</script>

<svelte:head>
  <title>{t('collab_groups.title')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<CollabNav {owner} {repo} current="groups" />

<div class="page-header"><h1>{t('collab_groups.title')}</h1></div>
<p class="intro">{t('collab_groups.intro')}</p>

<SectionCard title={t('collab_groups.create.title')} note={t('collab_groups.create.note')}>
  <GroupCreateForm {owner} {repo} {onCreated} />
</SectionCard>

<SectionCard title={t('collab_groups.open.title')} note={t('collab_groups.no_list_note')}>
  <form class="open" onsubmit={open}>
    <label>
      <span class="form-label">{t('collab_groups.open.id_label')}</span>
      <input class="form-input mono" bind:value={openId} autocomplete="off" />
    </label>
    <Button type="submit" disabled={!openId.trim()}>{t('collab_groups.open.button')}</Button>
  </form>

  <h3>{t('collab_groups.remembered.title')}</h3>
  <p class="muted">{t('collab_groups.remembered.note')}</p>
  {#if remembered.length === 0}
    <p class="muted" data-testid="remembered-empty">{t('collab_groups.remembered.empty')}</p>
  {:else}
    <ul class="list" data-testid="remembered-list">
      {#each remembered as g (g.id)}
        <li>
          <a class="text" href={groupHref(g.id)}>{g.title || g.id}</a>
          <Button size="sm" variant="ghost" onclick={() => drop(g.id)}>{t('collab_groups.remembered.forget')}</Button>
        </li>
      {/each}
    </ul>
  {/if}
</SectionCard>

<style>
  .intro { margin: -8px 0 20px; color: var(--text-secondary); font-size: 13px; max-width: 80ch; }
  .muted { color: var(--text-muted); font-size: 12px; margin: 4px 0; }
  h3 { margin: 16px 0 0; font-size: 14px; }
  .open { display: flex; gap: 10px; align-items: flex-end; flex-wrap: wrap; }
  .open label { display: flex; flex-direction: column; gap: 4px; flex: 1 1 300px; }
  .form-label { font-size: 13px; font-weight: 600; }
  .mono { font-family: var(--font-mono); font-size: 12px; }
  .list { list-style: none; margin: 0; padding: 0; }
  .list li { display: flex; align-items: center; gap: 10px; padding: 6px 0; border-top: 1px solid var(--border-light); }
  .text { overflow-wrap: anywhere; min-width: 0; flex: 1 1 auto; }
</style>
