<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import { createT, formatDateTime } from '$lib/i18n';
  import { confirmDialog } from '$lib/components/confirm.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import FormField from '$lib/components/FormField.svelte';
  import { page } from '$app/stores';
  import { ciSecrets, type CiSecret } from '$lib/api/client.svelte';

  const t = createT();
  const owner = $derived($page.params.owner!);
  const repo = $derived($page.params.repo!);

  let items = $state<CiSecret[]>([]);
  let name = $state('');
  let value = $state('');
  let error = $state('');
  let saving = $state(false);

  // Mirrors the server's rule in rg-http::api::ci_secrets::valid_secret_name.
  // The input accepts lowercase (the viewer should not have to shout), and the
  // name is upper-cased on submit; the pattern is the server's shape applied
  // case-insensitively so a name can never reach the API in a form it rejects.
  const NAME_PATTERN = '[A-Za-z_][A-Za-z0-9_]*';
  const NAME_MAX_LENGTH = 100;
  // Names the CI runner injects itself; the server rejects them.
  const RESERVED_NAMES = new Set([
    'CI',
    'IRONFORGE',
    'CI_PIPELINE_ID',
    'CI_COMMIT_SHA',
    'CI_SHA',
    'CI_REF',
    'CI_EVENT',
    'CI_JOB_TOKEN',
    'CI_OIDC_TOKEN_URL',
    'HOME',
    'PATH',
  ]);

  const normalizedName = $derived(name.trim().toUpperCase());
  const nameHint = $derived(
    normalizedName ? t('settings.ci_secrets.name_hint', { name: normalizedName }) : ''
  );

  $effect(() => {
    owner;
    repo;
    load();
  });

  async function load() {
    try {
      items = await ciSecrets.list(owner, repo);
      error = '';
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    }
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    const secretName = normalizedName;
    if (RESERVED_NAMES.has(secretName)) {
      error = t('settings.ci_secrets.name_reserved', { name: secretName });
      return;
    }
    try {
      saving = true;
      error = '';
      await ciSecrets.put(owner, repo, secretName, value);
      name = '';
      value = '';
      await load();
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      saving = false;
    }
  }

  async function remove(item: CiSecret) {
    if (!(await confirmDialog({ variant: 'danger', message: t('confirm.ci_secret_delete', { name: item.name }) }))) return;
    try {
      await ciSecrets.delete(owner, repo, item.name);
      await load();
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    }
  }
</script>

<svelte:head><title>{t('settings.ci_secrets.title')} · {owner}/{repo}</title></svelte:head>

<div class="settings-page">
  <header>
    <h1>{t('settings.ci_secrets.title')}</h1>
    <p>{t('settings.ci_secrets.desc')}</p>
  </header>

  {#if error}<ErrorBanner message={error} />{/if}

  <section>
    <h2>{t('settings.ci_secrets.add_title')}</h2>
    <form onsubmit={save}>
      <FormField id="secret-name" label={t('settings.ci_secrets.name')} hint={nameHint}>
        {#snippet children({ id, describedBy, invalid })}
          <input
            {id}
            bind:value={name}
            pattern={NAME_PATTERN}
            maxlength={NAME_MAX_LENGTH}
            placeholder="DEPLOY_TOKEN"
            aria-describedby={describedBy}
            aria-invalid={invalid ? 'true' : undefined}
            required
          />
        {/snippet}
      </FormField>
      <FormField id="secret-value" label={t('settings.ci_secrets.value')}>
        {#snippet children({ id })}
          <input {id} type="password" bind:value={value} minlength="4" maxlength="65536" required />
        {/snippet}
      </FormField>
      <div class="form-actions">
        <button class="btn btn-primary" disabled={saving}>{t('settings.ci_secrets.save')}</button>
      </div>
    </form>
  </section>

  <section>
    <h2>{t('settings.ci_secrets.configured')}</h2>
    {#if items.length === 0}
      <EmptyState title={t('settings.ci_secrets.empty')} />
    {:else}
      <div class="list">
        {#each items as item (item.name)}
          <article class="surface">
            <div>
              <strong>{item.name}</strong>
              <small>{t('common.updated', { date: formatDateTime(item.updated_at) })}</small>
            </div>
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

  header p,
  small {
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

  small {
    display: block;
    margin-top: 5px;
  }
</style>