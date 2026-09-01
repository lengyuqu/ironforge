<script lang="ts">
  import { webhooks, type RepositoryWebhook } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    repo,
    hooks,
    onChanged,
  }: {
    owner: string;
    repo: string;
    hooks: RepositoryWebhook[];
    onChanged: () => void | Promise<void>;
  } = $props();

  let deletingId = $state<number | null>(null);

  async function setActive(hook: RepositoryWebhook, nextActive: boolean) {
    try {
      await webhooks.update(owner, repo, hook.id, { active: nextActive });
      toast.success(t('settings.webhooks.updated', 'Webhook updated.'));
      await onChanged();
    } catch (e) {
      toast.error(toErrorMessage(e, t('settings.webhooks.update_failed', 'Failed to update webhook')));
    }
  }

  async function removeWebhook(hook: RepositoryWebhook) {
    if (!confirm(t('settings.webhooks.delete_confirm', `Delete webhook ${hook.url}?`))) return;

    deletingId = hook.id;
    try {
      await webhooks.remove(owner, repo, hook.id);
      toast.success(t('settings.webhooks.deleted', 'Webhook deleted.'));
      await onChanged();
    } catch (e) {
      toast.error(toErrorMessage(e, t('settings.webhooks.delete_failed', 'Failed to delete webhook')));
    } finally {
      deletingId = null;
    }
  }

  function eventList(hook: RepositoryWebhook) {
    return hook.events.split(',').map((event) => event.trim()).filter(Boolean).join(', ');
  }
</script>

{#if hooks.length === 0}
  <div class="empty-state">{t('settings.webhooks.empty', 'No webhooks configured yet.')}</div>
{:else}
  <div class="hook-list">
    {#each hooks as hook (hook.id)}
      <article class="hook-item">
        <div class="hook-main">
          <div class="hook-url">{hook.url}</div>
          <div class="hook-meta">
            <span>{hook.content_type}</span>
            <span>{eventList(hook)}</span>
          </div>
        </div>
        <div class="hook-actions">
          <label class="checkbox-row compact">
            <input type="checkbox" checked={hook.active} onchange={(e) => setActive(hook, e.currentTarget.checked)} />
            <span>{hook.active ? t('settings.webhooks.enabled', 'Enabled') : t('settings.webhooks.disabled', 'Disabled')}</span>
          </label>
          <button class="btn btn-danger" type="button" onclick={() => removeWebhook(hook)} disabled={deletingId === hook.id}>
            {deletingId === hook.id ? t('common.loading') : t('common.delete')}
          </button>
        </div>
      </article>
    {/each}
  </div>
{/if}

<style>
  .hook-list {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .hook-item {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    padding: 1rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-secondary);
  }

  .hook-main {
    min-width: 0;
  }

  .hook-url {
    overflow-wrap: anywhere;
    color: var(--text-primary);
    font-weight: 600;
  }

  .hook-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem;
    margin-top: 0.4rem;
    color: var(--text-secondary);
    font-size: 0.85rem;
  }

  .hook-actions {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex-shrink: 0;
  }

  .checkbox-row.compact {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    white-space: nowrap;
    color: var(--text-primary);
    font-weight: 400;
  }

  .btn {
    padding: 0.6rem 1.25rem;
    border: none;
    border-radius: 6px;
    font-size: 0.9rem;
    font-weight: 500;
    cursor: pointer;
  }

  .btn-danger {
    background: var(--red, #ff4444);
    color: white;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .empty-state {
    padding: 2rem;
    text-align: center;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border-radius: 6px;
  }

  @media (max-width: 720px) {
    .hook-item,
    .hook-actions {
      display: flex;
      flex-direction: column;
      align-items: stretch;
    }
  }
</style>
