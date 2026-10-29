<script lang="ts">
  import { createT } from '$lib/i18n';
  import type { AdminSsoProvider } from '$lib/api/client.svelte';

  interface Props {
    providers: AdminSsoProvider[];
    /** Provider whose connection test is running, if any. */
    testingId: number | null;
    /** Outcome of the last connection test. */
    testResult: { ok: boolean; message: string } | null;
    onTest: (provider: AdminSsoProvider) => void;
    onToggle: (provider: AdminSsoProvider) => void;
    onEdit: (provider: AdminSsoProvider) => void;
    onDelete: (provider: AdminSsoProvider) => void;
  }

  let { providers, testingId, testResult, onTest, onToggle, onEdit, onDelete }: Props = $props();

  const t = createT();
</script>

{#if testResult}
  <div class="connection-result" class:success={testResult.ok}>
    {testResult.message}
  </div>
{/if}

{#if providers.length === 0}
  <p class="text-secondary">{t('admin.sso.empty')}</p>
{:else}
  <div class="provider-list">
    {#each providers as provider (provider.id)}
      <div class="provider-row">
        <div>
          <strong>{provider.name}</strong>
          <div class="provider-meta">
            <span>{provider.slug}</span>
            <span>{provider.provider_type}</span>
            <span class:enabled={provider.enabled}>{provider.enabled ? t('admin.sso.status_enabled') : t('admin.sso.status_disabled')}</span>
          </div>
        </div>
        <div class="provider-actions">
          {#if provider.provider_type === 'ldap'}
            <button
              class="btn-secondary"
              type="button"
              disabled={testingId === provider.id}
              onclick={() => onTest(provider)}
            >
              {testingId === provider.id ? t('admin.sso.testing') : t('admin.sso.test_connection')}
            </button>
          {/if}
          <button class="btn-secondary" type="button" onclick={() => onToggle(provider)}>
            {provider.enabled ? t('admin.sso.disable') : t('admin.sso.enable')}
          </button>
          <button class="btn-secondary" type="button" onclick={() => onEdit(provider)}>{t('common.edit')}</button>
          <button class="btn-danger" type="button" onclick={() => onDelete(provider)}>{t('common.delete')}</button>
        </div>
      </div>
    {/each}
  </div>
{/if}

<style>
  .provider-list { display: flex; flex-direction: column; gap: 8px; margin-bottom: 16px; }

  .connection-result {
    margin-bottom: 12px;
    padding: 8px 10px;
    border: 1px solid var(--red);
    border-radius: var(--radius);
    color: var(--red);
    font-size: 13px;
  }
  .connection-result.success { border-color: var(--green-dim); color: var(--green); }

  .provider-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-primary);
  }

  .provider-meta { display: flex; flex-wrap: wrap; gap: 8px; color: var(--text-secondary); font-size: 12px; margin-top: 4px; }
  .provider-meta .enabled { color: var(--green); }
  .provider-actions { display: flex; flex-wrap: wrap; gap: 6px; justify-content: flex-end; }

  @media (max-width: 720px) {
    .provider-row { align-items: stretch; flex-direction: column; }
    .provider-actions { justify-content: flex-start; }
  }
</style>
