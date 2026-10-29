<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';
  import { confirmDialog } from '$lib/components/confirm.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import FormField from '$lib/components/FormField.svelte';
  import { page } from '$app/stores';
  import { tagProtections, type TagProtection } from '$lib/api/client.svelte';

  const t = createT();
  const owner = $derived($page.params.owner!);
  const repo = $derived($page.params.repo!);

  let items = $state<TagProtection[]>([]);
  let pattern = $state('');
  let error = $state('');

  $effect(() => {
    owner;
    repo;
    load();
  });

  async function load() {
    try {
      items = await tagProtections.list(owner, repo);
      error = '';
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    }
  }

  async function create(event: SubmitEvent) {
    event.preventDefault();
    try {
      await tagProtections.create(owner, repo, pattern.trim());
      pattern = '';
      await load();
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    }
  }

  async function remove(item: TagProtection) {
    if (!(await confirmDialog({ variant: 'danger', message: t('confirm.tag_protection_delete', { pattern: item.pattern }) }))) return;
    try {
      await tagProtections.delete(owner, repo, item.id);
      await load();
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    }
  }
</script>

<svelte:head><title>{t('settings.tag_protection.title')} · {owner}/{repo}</title></svelte:head>

<div class="settings-page">
  <header>
    <h1>{t('settings.tag_protection.title')}</h1>
    <p>{t('settings.tag_protection.desc')}</p>
  </header>

  {#if error}<ErrorBanner message={error} />{/if}

  <section>
    <h2>{t('settings.tag_protection.add_title')}</h2>
    <form onsubmit={create}>
      <FormField id="tag-pattern" label={t('settings.tag_protection.pattern')}>
        {#snippet children({ id })}
          <input {id} bind:value={pattern} maxlength="255" placeholder="v*" required />
        {/snippet}
      </FormField>
      <div class="form-actions">
        <button class="btn btn-primary">{t('settings.tag_protection.add')}</button>
      </div>
    </form>
  </section>

  <section>
    <h2>{t('settings.tag_protection.list_title')}</h2>
    {#if items.length === 0}
      <EmptyState title={t('settings.tag_protection.empty')} />
    {:else}
      <div class="list">
        {#each items as item (item.id)}
          <article class="surface">
            <code>{item.pattern}</code>
            <button class="btn btn-danger" onclick={() => remove(item)}>{t('common.delete')}</button>
          </article>
        {/each}
      </div>
    {/if}
  </section>
</div>

<style>
  .settings-page {
    max-width: 880px;
  }

  header,
  section {
    margin-bottom: 28px;
  }

  header p {
    color: var(--text-secondary);
  }

  .list {
    display: grid;
    gap: 10px;
  }

  article {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px;
  }
</style>
