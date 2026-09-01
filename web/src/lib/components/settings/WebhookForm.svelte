<script lang="ts">
  import { webhooks } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    repo,
    onCreated,
  }: {
    owner: string;
    repo: string;
    onCreated: () => void | Promise<void>;
  } = $props();

  const eventOptions = [
    'push',
    'issue.opened',
    'issue.closed',
    'issue.comment',
    'pull_request.opened',
    'pull_request.closed',
    'pull_request.merged',
    'release.created',
    'release.deleted',
    'branch.created',
    'branch.deleted',
    'tag.created',
    'tag.deleted',
    'milestone.closed',
  ];

  let url = $state('');
  let secret = $state('');
  let contentType = $state<'json' | 'form'>('json');
  let active = $state(true);
  let selectedEvents = $state<string[]>(['push']);
  let saving = $state(false);

  function toggleEvent(event: string, checked: boolean) {
    selectedEvents = checked
      ? Array.from(new Set([...selectedEvents, event]))
      : selectedEvents.filter((item) => item !== event);
  }

  async function createWebhook(event: SubmitEvent) {
    event.preventDefault();
    if (!url.trim()) {
      toast.error(t('settings.webhooks.url_required', 'Enter a payload URL.'));
      return;
    }
    if (selectedEvents.length === 0) {
      toast.error(t('settings.webhooks.events_required', 'Select at least one event.'));
      return;
    }

    saving = true;
    try {
      await webhooks.create(owner, repo, {
        url: url.trim(),
        content_type: contentType,
        secret: secret.trim() || undefined,
        active,
        events: selectedEvents,
      });
      url = '';
      secret = '';
      contentType = 'json';
      active = true;
      selectedEvents = ['push'];
      toast.success(t('settings.webhooks.created', 'Webhook created.'));
      await onCreated();
    } catch (e) {
      toast.error(toErrorMessage(e, t('settings.webhooks.create_failed', 'Failed to create webhook')));
    } finally {
      saving = false;
    }
  }
</script>

<form class="webhook-form" onsubmit={createWebhook}>
  <div class="form-group">
    <label for="webhook-url">{t('settings.webhooks.url', 'Payload URL')}</label>
    <input id="webhook-url" type="url" bind:value={url} placeholder="https://example.com/webhook" disabled={saving} />
  </div>

  <div class="form-row">
    <div class="form-group">
      <label for="webhook-content-type">{t('settings.webhooks.content_type', 'Content type')}</label>
      <select id="webhook-content-type" bind:value={contentType} disabled={saving}>
        <option value="json">application/json</option>
        <option value="form">application/x-www-form-urlencoded</option>
      </select>
    </div>

    <div class="form-group">
      <label for="webhook-secret">{t('settings.webhooks.secret', 'Secret')}</label>
      <input id="webhook-secret" type="password" bind:value={secret} autocomplete="new-password" disabled={saving} />
    </div>
  </div>

  <fieldset class="event-grid">
    <legend>{t('settings.webhooks.events', 'Events')}</legend>
    {#each eventOptions as event}
      <label>
        <input
          type="checkbox"
          checked={selectedEvents.includes(event)}
          disabled={saving}
          onchange={(e) => toggleEvent(event, e.currentTarget.checked)}
        />
        <span>{event}</span>
      </label>
    {/each}
  </fieldset>

  <label class="checkbox-row">
    <input type="checkbox" bind:checked={active} disabled={saving} />
    <span>{t('settings.webhooks.active', 'Active')}</span>
  </label>

  <div class="actions">
    <button class="btn btn-primary" type="submit" disabled={saving || !url.trim() || selectedEvents.length === 0}>
      {saving ? t('common.loading') : t('settings.webhooks.create', 'Add webhook')}
    </button>
  </div>
</form>

<style>
  .webhook-form {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .form-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 1rem;
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  label,
  legend {
    color: var(--text-primary);
    font-size: 0.9rem;
    font-weight: 500;
  }

  input,
  select {
    padding: 0.6rem 0.75rem;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 0.9rem;
  }

  input:focus,
  select:focus {
    outline: none;
    border-color: var(--accent);
  }

  .event-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 0.75rem;
    margin: 0;
    padding: 1rem;
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .event-grid label,
  .checkbox-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    color: var(--text-primary);
    font-weight: 400;
  }

  .actions {
    display: flex;
    gap: 0.75rem;
  }

  .btn {
    padding: 0.6rem 1.25rem;
    border: none;
    border-radius: 6px;
    font-size: 0.9rem;
    font-weight: 500;
    cursor: pointer;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-primary {
    background: var(--accent);
    color: white;
  }

  @media (max-width: 720px) {
    .form-row {
      display: flex;
      flex-direction: column;
      align-items: stretch;
    }
  }
</style>
