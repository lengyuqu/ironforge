<script lang="ts">
  import { orgs } from '$lib/api/client.svelte';
  import type { OrgSummary } from '$lib/types/entities';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    onCreated,
  }: {
    onCreated: (org: OrgSummary) => void | Promise<void>;
  } = $props();

  let name = $state('');
  let displayName = $state('');
  let description = $state('');
  let visibility = $state('public');
  let error = $state('');
  let creating = $state(false);

  function resetForm() {
    name = '';
    displayName = '';
    description = '';
    visibility = 'public';
  }

  async function handleCreate(e?: Event) {
    e?.preventDefault();
    if (!name.trim()) {
      error = t('errors.create_failed');
      return;
    }

    creating = true;
    error = '';
    try {
      const result = await orgs.create(
        name,
        displayName || undefined,
        description || undefined,
        visibility
      );
      resetForm();
      await onCreated(result);
    } catch (e: any) {
      error = toErrorMessage(e, t('errors.create_failed'));
    } finally {
      creating = false;
    }
  }
</script>

<form onsubmit={handleCreate} class="create-panel">
  <h2>{t('orgs.create_title')}</h2>

  {#if error}
    <div class="error" role="alert">{error}</div>
  {/if}

  <div class="field">
    <label for="name">{t('orgs.name')} *</label>
    <input
      id="name"
      type="text"
      bind:value={name}
      placeholder={t('orgs.name_placeholder')}
      required
    />
  </div>

  <div class="field">
    <label for="displayName">{t('orgs.display_name')}</label>
    <input
      id="displayName"
      type="text"
      bind:value={displayName}
      placeholder={t('orgs.display_name_placeholder')}
    />
  </div>

  <div class="field">
    <label for="description">{t('orgs.description')}</label>
    <textarea
      id="description"
      bind:value={description}
      placeholder={t('orgs.description_placeholder')}
      rows="3"></textarea>
  </div>

  <div class="field">
    <label for="visibility">{t('orgs.visibility')}</label>
    <select id="visibility" bind:value={visibility}>
      <option value="public">{t('orgs.visibility_public')}</option>
      <option value="private">{t('orgs.visibility_private')}</option>
    </select>
  </div>

  <div class="form-actions">
    <button type="submit" class="btn-primary" disabled={creating}>
      {creating ? t('orgs.submitting') : t('orgs.submit')}
    </button>
  </div>
</form>

<style>
  .create-panel {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1.25rem;
    display: flex;
    flex-direction: column;
    gap: 1rem;
    margin-bottom: 1.5rem;
  }

  h2 {
    color: var(--text-primary);
    font-size: 1.1rem;
    margin: 0;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  label {
    color: var(--text-secondary);
    font-size: 0.9rem;
    font-weight: 600;
  }

  input,
  textarea,
  select {
    background: var(--bg-primary);
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.5rem 0.75rem;
    font-size: 0.95rem;
  }

  input:focus,
  textarea:focus,
  select:focus {
    outline: none;
    border-color: var(--accent);
  }

  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.75rem;
  }

  .btn-primary {
    border: none;
    border-radius: 6px;
    padding: 0.6rem 1.2rem;
    font-size: 0.95rem;
    cursor: pointer;
    background: var(--accent);
    color: white;
  }

  .btn-primary:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .error {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    border: 1px solid rgba(248, 81, 73, 0.35);
    padding: 0.65rem 0.85rem;
    border-radius: 6px;
  }

  @media (max-width: 700px) {
    .form-actions {
      flex-direction: column-reverse;
    }

    .btn-primary {
      width: 100%;
    }
  }
</style>
