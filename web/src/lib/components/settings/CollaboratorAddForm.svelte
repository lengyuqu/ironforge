<script lang="ts">
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import { collaborators } from '$lib/api/client.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    repo,
    onAdded,
  }: {
    owner: string;
    repo: string;
    onAdded: () => void | Promise<void>;
  } = $props();

  let userIdentifier = $state('');
  let permission = $state<'read' | 'write' | 'admin'>('read');
  let adding = $state(false);
  let error = $state('');

  const permissionOptions = [
    { value: 'read', label: t('orgs.permission.read') },
    { value: 'write', label: t('orgs.permission.write') },
    { value: 'admin', label: t('orgs.permission.admin') }
  ];

  async function handleAdd(event: SubmitEvent) {
    event.preventDefault();

    const identifier = userIdentifier.trim();
    if (!identifier) {
      error = t('settings.collaborators.user_required');
      return;
    }

    try {
      adding = true;
      error = '';
      await collaborators.add(owner, repo, identifier, permission);
      userIdentifier = '';
      permission = 'read';
      await onAdded();
    } catch (err: any) {
      error = toErrorMessage(err, t('settings.collaborators.add_failed'));
    } finally {
      adding = false;
    }
  }
</script>

<section class="section">
  <h2>{t('settings.collaborators.add_title')}</h2>

  {#if error}
    <ErrorBanner message={error} />
  {/if}

  <form class="add-form" onsubmit={handleAdd}>
    <div class="form-group">
      <label for="collaborator-user">{t('settings.collaborators.user_identifier')}</label>
      <input
        id="collaborator-user"
        type="text"
        bind:value={userIdentifier}
        placeholder={t('settings.collaborators.user_placeholder')}
        disabled={adding}
      />
    </div>

    <div class="form-group">
      <label for="collaborator-permission">{t('settings.collaborators.permission')}</label>
      <select id="collaborator-permission" bind:value={permission} disabled={adding}>
        {#each permissionOptions as option (option.value)}
          <option value={option.value}>{option.label}</option>
        {/each}
      </select>
    </div>

    <button class="btn btn-primary" type="submit" disabled={adding || !userIdentifier.trim()}>
      {adding ? t('settings.collaborators.adding') : t('settings.collaborators.add')}
    </button>
  </form>
</section>

<style>
  h2 {
    font-size: 1.1rem;
    margin: 0 0 1rem;
    color: var(--text-primary);
  }

  .section {
    margin-bottom: 2.5rem;
    padding-bottom: 2rem;
    border-bottom: 1px solid var(--border);
  }

  .add-form {
    display: grid;
    grid-template-columns: minmax(160px, 1fr) minmax(140px, 180px) auto;
    align-items: end;
    gap: 1rem;
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  label {
    color: var(--text-primary);
    font-size: 0.9rem;
    font-weight: 500;
  }

  input,
  select {
    min-height: 38px;
    padding: 0.55rem 0.7rem;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 0.9rem;
  }

  input:focus,
  select:focus {
    border-color: var(--accent);
    outline: none;
  }

  @media (max-width: 760px) {
    .add-form {
      grid-template-columns: 1fr;
    }
  }
</style>
