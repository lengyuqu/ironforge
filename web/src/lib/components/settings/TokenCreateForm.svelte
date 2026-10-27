<script lang="ts">
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import { tokens } from '$lib/api/client.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    onCreated,
  }: {
    /** 创建成功后回调（父级刷新列表） */
    onCreated: () => void | Promise<void>;
  } = $props();

  let name = $state('');
  let scopes = $state('repo');
  let expiresAt = $state('');
  let creating = $state(false);
  let error = $state('');
  let newToken = $state('');
  let copied = $state(false);

  function expiresAtIso() {
    if (!expiresAt) return undefined;
    const parsed = new Date(`${expiresAt}T23:59:59`);
    return Number.isNaN(parsed.getTime()) ? undefined : parsed.toISOString();
  }

  async function createToken(event: SubmitEvent) {
    event.preventDefault();
    if (!name.trim()) {
      error = t('settings.tokens.name_required', 'Token name is required');
      return;
    }

    try {
      creating = true;
      error = '';
      newToken = '';
      copied = false;
      const created = await tokens.create(name.trim(), scopes.trim() || 'repo', expiresAtIso());
      newToken = created.token;
      name = '';
      scopes = 'repo';
      expiresAt = '';
      await onCreated();
    } catch (err: any) {
      error = toErrorMessage(err, t('errors.create_failed'));
    } finally {
      creating = false;
    }
  }

  async function copyNewToken() {
    if (!newToken) return;
    await navigator.clipboard.writeText(newToken);
    copied = true;
  }
</script>

{#if newToken}
  <section class="token-created surface" aria-label="New access token">
    <div>
      <strong>{t('settings.tokens.created_title', 'New token')}</strong>
      <p>{t('settings.tokens.created_hint', 'Copy this value before leaving the page.')}</p>
    </div>
    <code>{newToken}</code>
    <button type="button" class="btn btn-primary" onclick={copyNewToken}>
      {copied ? t('common.copied', 'Copied') : t('common.copy', 'Copy')}
    </button>
  </section>
{/if}

<section class="settings-section">
  <h2>{t('settings.tokens.create_title', 'Create Token')}</h2>

  {#if error}
    <ErrorBanner message={error} />
  {/if}

  <form class="create-form" onsubmit={createToken}>
    <label>
      {t('settings.tokens.name_label', 'Name')}
      <input bind:value={name} placeholder="CI deploy token" disabled={creating} />
    </label>
    <label>
      {t('settings.tokens.scopes_label', 'Scopes')}
      <input bind:value={scopes} placeholder="repo" disabled={creating} />
    </label>
    <label>
      {t('settings.tokens.expires_label', 'Expires')}
      <input type="date" bind:value={expiresAt} disabled={creating} />
    </label>
    <button type="submit" class="btn btn-primary" disabled={creating || !name.trim()}>
      {creating ? t('settings.tokens.creating', 'Creating...') : t('settings.tokens.create', 'Create')}
    </button>
  </form>
</section>

<style>
  h2 {
    margin: 0 0 16px;
  }

  .create-form {
    display: grid;
    grid-template-columns: minmax(180px, 1.2fr) minmax(140px, 0.8fr) minmax(150px, 0.8fr) auto;
    align-items: end;
    gap: 12px;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 600;
  }

  input {
    min-height: 36px;
  }

  .token-created {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 12px;
    align-items: center;
    padding: 14px 16px;
    margin-bottom: 20px;
  }

  .token-created code {
    grid-column: 1 / -1;
    display: block;
    padding: 10px;
    overflow-x: auto;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .token-created p {
    margin: 0;
    color: var(--text-secondary);
  }

  @media (max-width: 720px) {
    .create-form {
      grid-template-columns: 1fr;
    }

    .token-created {
      grid-template-columns: 1fr;
    }
  }
</style>
