<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import SuccessBanner from '$lib/components/SuccessBanner.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { confirmDialog } from '$lib/components/confirm.svelte';
  import { goto } from '$app/navigation';
  import {
    remoteCredentials,
    type RemoteCredential,
    type RemoteCredentialPayload,
  } from '$lib/api/remote-credentials';
  import { createT } from '$lib/i18n';
  import { isAuthReady, isLoggedIn } from '$lib/stores/auth.svelte';

  const t = createT();

  let items = $state<RemoteCredential[]>([]);
  let loading = $state(true);
  let saving = $state(false);
  let deletingId = $state<number | null>(null);
  let error = $state('');
  let success = $state('');

  let host = $state('');
  let username = $state('');
  let token = $state('');
  let authScheme = $state<'basic' | 'bearer'>('basic');
  let label = $state('');

  $effect(() => {
    if (!isAuthReady()) return;
    if (!isLoggedIn()) {
      goto('/login');
      return;
    }
    load();
  });

  async function load() {
    try {
      loading = true;
      error = '';
      items = await remoteCredentials.list();
    } catch (err) {
      error = toErrorMessage(err, t('remote_credentials.load_failed'));
    } finally {
      loading = false;
    }
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!host.trim()) {
      error = t('remote_credentials.host_required');
      return;
    }
    if (!token.trim()) {
      error = t('remote_credentials.token_required');
      return;
    }

    const payload: RemoteCredentialPayload = {
      host: host.trim(),
      token: token.trim(),
      auth_scheme: authScheme,
    };
    if (username.trim()) payload.username = username.trim();
    if (label.trim()) payload.label = label.trim();

    try {
      saving = true;
      error = '';
      success = '';
      await remoteCredentials.create(payload);
      host = '';
      username = '';
      token = '';
      label = '';
      authScheme = 'basic';
      success = t('remote_credentials.saved');
      await load();
    } catch (err) {
      error = toErrorMessage(err, t('remote_credentials.save_failed'));
    } finally {
      saving = false;
    }
  }

  async function remove(item: RemoteCredential) {
    if (
      !(await confirmDialog({
        variant: 'danger',
        message: t('remote_credentials.delete_confirm', { host: item.host }),
      }))
    )
      return;

    try {
      deletingId = item.id;
      error = '';
      success = '';
      await remoteCredentials.remove(item.id);
      success = t('remote_credentials.deleted');
      await load();
    } catch (err) {
      error = toErrorMessage(err, t('remote_credentials.delete_failed'));
    } finally {
      deletingId = null;
    }
  }

  function formatDate(value?: string | null) {
    if (!value) return t('remote_credentials.never');
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
  }
</script>

<svelte:head>
  <title>{t('remote_credentials.title')} · IronForge</title>
</svelte:head>

<div class="remote-credentials-page">
  <header class="page-header">
    <h1>{t('remote_credentials.title')}</h1>
    <p>{t('remote_credentials.description')}</p>
  </header>

  {#if error}<div class="message error-box" role="alert">{error}</div>{/if}
  {#if success}<SuccessBanner message={success} />{/if}

  <section class="settings-section">
    <h2>{t('remote_credentials.add_title')}</h2>
    <p class="hint">{t('remote_credentials.add_hint')}</p>
    <form class="create-form" onsubmit={submit}>
      <label for="rc-host">{t('remote_credentials.host')}</label>
      <input
        id="rc-host"
        bind:value={host}
        placeholder="github.com"
        disabled={saving}
        maxlength="255"
        spellcheck="false"
        autocomplete="off"
      />

      <label for="rc-label">{t('remote_credentials.label')}</label>
      <input
        id="rc-label"
        bind:value={label}
        placeholder={t('remote_credentials.label_placeholder')}
        disabled={saving}
        maxlength="128"
      />

      <label for="rc-scheme">{t('remote_credentials.auth_scheme')}</label>
      <select id="rc-scheme" bind:value={authScheme} disabled={saving}>
        <option value="basic">{t('remote_credentials.scheme_basic')}</option>
        <option value="bearer">{t('remote_credentials.scheme_bearer')}</option>
      </select>

      {#if authScheme === 'basic'}
        <label for="rc-username">{t('remote_credentials.username')}</label>
        <input
          id="rc-username"
          bind:value={username}
          placeholder="x-access-token"
          disabled={saving}
          maxlength="255"
          spellcheck="false"
          autocomplete="off"
        />
      {/if}

      <label for="rc-token">{t('remote_credentials.token')}</label>
      <input
        id="rc-token"
        type="password"
        bind:value={token}
        placeholder={t('remote_credentials.token_placeholder')}
        disabled={saving}
        spellcheck="false"
        autocomplete="new-password"
      />

      <div>
        <button class="btn btn-primary" type="submit" disabled={saving}>
          {saving ? t('remote_credentials.saving') : t('remote_credentials.save')}
        </button>
      </div>
    </form>
  </section>

  <section class="settings-section">
    <h2>{t('remote_credentials.existing')}</h2>
    {#if loading}
      <p class="muted">{t('remote_credentials.loading')}</p>
    {:else if items.length === 0}
      <EmptyState title={t('remote_credentials.empty')} />
    {:else}
      <div class="credential-list">
        {#each items as item (item.id)}
          <article class="credential-card">
            <div class="credential-main">
              <h3>
                {item.label || item.host}
                <span class="badge">{item.auth_scheme}</span>
              </h3>
              <code>{item.host}</code>
              <dl>
                <div>
                  <dt>{t('remote_credentials.token')}</dt>
                  <dd>{item.token_hint ?? t('remote_credentials.token_unreadable')}</dd>
                </div>
                <div>
                  <dt>{t('remote_credentials.username')}</dt>
                  <dd>{item.username ?? '—'}</dd>
                </div>
                <div>
                  <dt>{t('remote_credentials.last_used')}</dt>
                  <dd>{formatDate(item.last_used_at)}</dd>
                </div>
              </dl>
            </div>
            <button
              class="btn btn-danger"
              type="button"
              disabled={deletingId === item.id}
              onclick={() => remove(item)}
            >
              {deletingId === item.id
                ? t('remote_credentials.deleting')
                : t('remote_credentials.delete')}
            </button>
          </article>
        {/each}
      </div>
    {/if}
  </section>
</div>

<style>
  .remote-credentials-page { max-width: 880px; }
  h1 { margin: 0 0 6px; }
  h2 { margin: 0 0 16px; }
  h3 { display: flex; align-items: center; gap: 8px; margin: 0; }
  p { margin: 0; color: var(--text-secondary); }
  .hint { margin: -8px 0 16px; font-size: 13px; }
  .badge {
    padding: 2px 8px;
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--text-secondary);
    font-size: 11px;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .create-form { display: grid; gap: 8px; }
  label { margin-top: 8px; color: var(--text-secondary); font-size: 13px; font-weight: 600; }
  input, select { width: 100%; }
  .message { margin-bottom: 20px; padding: 14px 16px; border: 1px solid var(--border); border-radius: var(--radius); }
  .error-box { color: var(--red); background: color-mix(in srgb, var(--red) 10%, transparent); }
  .credential-list { display: grid; gap: 12px; }
  .credential-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .credential-main { min-width: 0; }
  code { display: block; margin-top: 8px; overflow-wrap: anywhere; color: var(--text-secondary); }
  dl { display: flex; flex-wrap: wrap; gap: 20px; margin: 12px 0 0; }
  dl div { display: flex; gap: 6px; }
  dt { color: var(--text-secondary); }
  dd { margin: 0; }
  .muted { color: var(--text-secondary); }
  @media (max-width: 600px) {
    .credential-card { align-items: flex-start; flex-direction: column; }
  }
</style>
