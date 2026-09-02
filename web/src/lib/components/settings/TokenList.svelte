<script lang="ts">
  import { tokens, type AccessToken } from '$lib/api/client.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    tokenList,
    loading,
    onRefresh,
  }: {
    tokenList: AccessToken[];
    loading: boolean;
    onRefresh: () => void | Promise<void>;
  } = $props();

  let deletingId = $state<number | null>(null);
  let error = $state('');

  async function revokeToken(token: AccessToken) {
    if (!confirm(t('settings.tokens.revoke_confirm', { name: token.name }))) return;

    try {
      deletingId = token.id;
      error = '';
      await tokens.delete(token.id);
      await onRefresh();
    } catch (err: any) {
      error = toErrorMessage(err, t('errors.revoke_failed', 'Revoke failed'));
    } finally {
      deletingId = null;
    }
  }

  function formatDate(value?: string | null) {
    if (!value) return t('settings.tokens.never_used', 'Never');
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return value;
    return date.toLocaleDateString();
  }
</script>

<section class="section">
  <h2>{t('settings.tokens.existing_title', 'Existing Tokens')}</h2>

  {#if error}
    <div class="error-box">{error}</div>
  {/if}

  {#if loading}
    <p class="muted">{t('common.loading')}</p>
  {:else if tokenList.length === 0}
    <div class="empty-state">{t('settings.tokens.empty', 'No personal access tokens yet.')}</div>
  {:else}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>{t('settings.tokens.name_label', 'Name')}</th>
            <th>{t('settings.tokens.scopes_label', 'Scopes')}</th>
            <th>{t('settings.tokens.created_label', 'Created')}</th>
            <th>{t('settings.tokens.last_used_label', 'Last used')}</th>
            <th>{t('settings.tokens.expires_label', 'Expires')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each tokenList as token (token.id)}
            <tr>
              <td>{token.name}</td>
              <td><code>{token.scopes}</code></td>
              <td>{formatDate(token.created_at)}</td>
              <td>{formatDate(token.last_used_at)}</td>
              <td>{formatDate(token.expires_at)}</td>
              <td class="actions">
                <button
                  type="button"
                  class="btn btn-danger"
                  disabled={deletingId === token.id}
                  onclick={() => revokeToken(token)}
                >
                  {deletingId === token.id
                    ? t('settings.tokens.revoking', 'Revoking...')
                    : t('settings.tokens.revoke', 'Revoke')}
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<style>
  h2 {
    margin: 0 0 16px;
    font-size: 18px;
  }

  .section {
    margin-bottom: 32px;
    padding-bottom: 28px;
    border-bottom: 1px solid var(--border);
  }

  .table-wrap {
    overflow-x: auto;
  }

  table {
    width: 100%;
    border-collapse: collapse;
  }

  th,
  td {
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
    text-align: left;
    vertical-align: middle;
  }

  th {
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
  }

  .actions {
    text-align: right;
  }

  .muted {
    color: var(--text-secondary);
  }

  .empty-state {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 14px 16px;
    margin-bottom: 20px;
    color: var(--text-secondary);
  }

  .error-box {
    color: var(--red);
    background: color-mix(in srgb, var(--red) 10%, transparent);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 14px 16px;
    margin-bottom: 20px;
  }

  .btn {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    cursor: pointer;
    font-size: 14px;
  }

  .btn-danger {
    padding: 5px 12px;
    background: color-mix(in srgb, var(--red) 12%, transparent);
    border-color: color-mix(in srgb, var(--red) 45%, transparent);
    color: var(--red);
  }

  .btn-danger:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
