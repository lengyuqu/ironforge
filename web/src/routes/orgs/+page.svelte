<script lang="ts">
  import { orgs } from '$lib/api/client';
  import { goto } from '$app/navigation';
  import { createT } from '$lib/i18n';

  const t = createT();

  let name = $state('');
  let displayName = $state('');
  let description = $state('');
  let visibility = $state('public');
  let error = $state('');
  let loading = $state(false);

  async function handleCreate(e?: Event) {
    e?.preventDefault();
    if (!name.trim()) { error = t('errors.create_failed'); return; }
    loading = true;
    error = '';
    try {
      const result = await orgs.create(name, displayName || undefined, description || undefined, visibility);
      goto(`/orgs/${result.name}`);
    } catch (e: any) {
      error = e.message || t('errors.create_failed');
    } finally {
      loading = false;
    }
  }
</script>

<div class="container">
  <h1>{t('orgs.create_title')}</h1>

  {#if error}
    <div class="error">{error}</div>
  {/if}

  <form onsubmit={(e) => { e.preventDefault(); handleCreate(e); }} class="form">
    <div class="field">
      <label for="name">{t('orgs.name')} *</label>
      <input id="name" type="text" bind:value={name} placeholder={t('orgs.name_placeholder')} required />
    </div>

    <div class="field">
      <label for="displayName">{t('orgs.display_name')}</label>
      <input id="displayName" type="text" bind:value={displayName} placeholder={t('orgs.display_name_placeholder')} />
    </div>

    <div class="field">
      <label for="description">{t('orgs.description')}</label>
      <textarea id="description" bind:value={description} placeholder={t('orgs.description_placeholder')} rows="3"></textarea>
    </div>

    <div class="field">
      <label for="visibility">{t('orgs.visibility')}</label>
      <select id="visibility" bind:value={visibility}>
        <option value="public">{t('orgs.visibility_public')}</option>
        <option value="private">{t('orgs.visibility_private')}</option>
      </select>
    </div>

    <button type="submit" class="btn-primary" disabled={loading}>
      {loading ? t('orgs.submitting') : t('orgs.submit')}
    </button>
  </form>
</div>

<style>
  .container { max-width: 600px; margin: 2rem auto; padding: 0 1rem; }
  h1 { color: var(--text-primary); margin-bottom: 1.5rem; }
  .form { display: flex; flex-direction: column; gap: 1rem; }
  .field { display: flex; flex-direction: column; gap: 0.3rem; }
  label { color: var(--text-secondary); font-size: 0.9rem; font-weight: 600; }
  input, textarea, select {
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.5rem 0.75rem;
    font-size: 0.95rem;
  }
  input:focus, textarea:focus, select:focus { outline: none; border-color: var(--accent); }
  .btn-primary {
    background: var(--accent);
    color: white;
    border: none;
    border-radius: 6px;
    padding: 0.6rem 1.2rem;
    font-size: 0.95rem;
    cursor: pointer;
    margin-top: 0.5rem;
  }
  .btn-primary:disabled { opacity: 0.6; cursor: not-allowed; }
  .error { color: #f85149; background: rgba(248, 81, 73, 0.1); padding: 0.5rem 0.75rem; border-radius: 6px; }
</style>
