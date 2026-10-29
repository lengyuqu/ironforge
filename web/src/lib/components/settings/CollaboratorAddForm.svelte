<script lang="ts">
  import FormField from '$lib/components/FormField.svelte';
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
    } catch (err) {
      error = toErrorMessage(err, t('settings.collaborators.add_failed'));
    } finally {
      adding = false;
    }
  }
</script>

<section class="settings-section">
  <h2>{t('settings.collaborators.add_title')}</h2>

  {#if error}
    <ErrorBanner message={error} />
  {/if}

  <form class="add-form" onsubmit={handleAdd}>
    <FormField id="collaborator-user" label={t('settings.collaborators.user_identifier')}>
      {#snippet children()}
        <input
          id="collaborator-user"
          type="text"
          bind:value={userIdentifier}
          placeholder={t('settings.collaborators.user_placeholder')}
          disabled={adding}
        />
      {/snippet}
    </FormField>

    <FormField id="collaborator-permission" label={t('settings.collaborators.permission')}>
      {#snippet children()}
        <select id="collaborator-permission" bind:value={permission} disabled={adding}>
          {#each permissionOptions as option (option.value)}
            <option value={option.value}>{option.label}</option>
          {/each}
        </select>
      {/snippet}
    </FormField>

    <button class="btn btn-primary" type="submit" disabled={adding || !userIdentifier.trim()}>
      {adding ? t('settings.collaborators.adding') : t('settings.collaborators.add')}
    </button>
  </form>
</section>

<style>
  h2 {
    margin: 0 0 1rem;
    color: var(--text-primary);
  }

  .add-form {
    display: grid;
    grid-template-columns: minmax(160px, 1fr) minmax(140px, 180px) auto;
    align-items: end;
    gap: 1rem;
  }

  input,
  select {
    min-height: 38px;
  }

  @media (max-width: 720px) {
    .add-form {
      grid-template-columns: 1fr;
    }
  }
</style>
