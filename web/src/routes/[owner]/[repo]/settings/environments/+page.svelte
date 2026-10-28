<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';
  import { confirmDialog } from '$lib/components/confirm.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import FormField from '$lib/components/FormField.svelte';
  import { page } from '$app/stores';
  import { ciEnvironments, type CiEnvironment } from '$lib/api/client.svelte';

  const t = createT();
  const owner = $derived($page.params.owner!);
  const repo = $derived($page.params.repo!);

  let items = $state<CiEnvironment[]>([]);
  let name = $state('production');
  let isProtected = $state(true);
  let required = $state(1);
  let approverIds = $state('');
  let error = $state('');
  let editingId = $state<number | null>(null);

  $effect(() => {
    owner;
    repo;
    load();
  });

  async function load() {
    try {
      items = await ciEnvironments.list(owner, repo);
      error = '';
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    }
  }

  function payload() {
    return {
      name: name.trim(),
      protected: isProtected,
      required_approvals: required,
      allowed_approver_ids: approverIds.split(',').map((v) => Number(v.trim())).filter(Number.isSafeInteger),
    };
  }

  function resetForm() {
    editingId = null;
    name = '';
    isProtected = true;
    required = 1;
    approverIds = '';
  }

  function edit(item: CiEnvironment) {
    editingId = item.id;
    name = item.name;
    isProtected = item.protected;
    required = item.required_approvals;
    approverIds = item.allowed_approver_ids.join(', ');
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    try {
      if (editingId === null) await ciEnvironments.create(owner, repo, payload());
      else await ciEnvironments.update(owner, repo, editingId, payload());
      resetForm();
      await load();
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    }
  }

  async function remove(item: CiEnvironment) {
    if (!(await confirmDialog({ variant: 'danger', message: t('confirm.ci_env_delete', { name: item.name }) }))) return;
    try {
      await ciEnvironments.delete(owner, repo, item.id);
      await load();
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    }
  }
</script>

<svelte:head><title>{t('settings.environments.title')} · {owner}/{repo}</title></svelte:head>

<div class="settings-page">
  <header>
    <h1>{t('settings.environments.heading')}</h1>
    <p>{t('settings.environments.desc')}</p>
  </header>

  {#if error}<ErrorBanner message={error} />{/if}

  <section>
    <h2>{editingId === null ? t('settings.environments.create_title') : t('settings.environments.edit_title')}</h2>
    <form onsubmit={save}>
      <FormField id="environment-name" label={t('settings.environments.name')}>
        {#snippet children({ id })}
          <input {id} bind:value={name} maxlength="255" required />
        {/snippet}
      </FormField>
      <label class="checkbox-label">
        <input type="checkbox" bind:checked={isProtected} /> {t('settings.environments.require_approval')}
      </label>
      <FormField id="required-approvals" label={t('settings.environments.required_approvals')}>
        {#snippet children({ id })}
          <input {id} type="number" min="1" max="10" bind:value={required} required />
        {/snippet}
      </FormField>
      <FormField id="approver-ids" label={t('settings.environments.approver_ids')} hint={t('settings.environments.approver_ids_hint')}>
        {#snippet children({ id, describedBy })}
          <input {id} aria-describedby={describedBy} bind:value={approverIds} placeholder="12, 34" />
        {/snippet}
      </FormField>
      <div class="form-actions">
        <button class="btn btn-primary">
          {editingId === null ? t('settings.environments.create_title') : t('settings.environments.save_changes')}
        </button>
        {#if editingId !== null}
          <button type="button" class="btn" onclick={resetForm}>{t('common.cancel')}</button>
        {/if}
      </div>
    </form>
  </section>

  <section>
    <h2>{t('settings.environments.configured')}</h2>
    {#if items.length === 0}
      <EmptyState title={t('settings.environments.empty')} />
    {:else}
      <div class="list">
        {#each items as item (item.id)}
          <article class="surface">
            <div>
              <strong>{item.name}</strong>
              <p>
                {item.protected
                  ? t('settings.environments.approvals_required', { count: item.required_approvals })
                  : t('settings.environments.unprotected')}{item.allowed_approver_ids.length
                  ? ` · ${t('settings.environments.reviewers', { ids: item.allowed_approver_ids.join(', ') })}`
                  : ''}
              </p>
            </div>
            <div class="actions">
              <button class="btn" onclick={() => edit(item)}>{t('common.edit')}</button>
              <button class="btn btn-danger" onclick={() => remove(item)}>{t('common.delete')}</button>
            </div>
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

  header p,
  article p {
    color: var(--text-secondary);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
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

  article p {
    margin: 4px 0 0;
    font-size: 13px;
  }
</style>
