<script lang="ts">
  import FormField from '$lib/components/FormField.svelte';
  import SuccessBanner from '$lib/components/SuccessBanner.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import { confirmDialog } from '$lib/components/confirm.svelte';
  import { mirrors, type RepositoryMirror } from '$lib/api/client.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    repo,
    mirror,
    onChanged,
  }: {
    owner: string;
    repo: string;
    mirror: RepositoryMirror | null;
    /** 保存/同步/删除成功后通知父级重载镜像状态 */
    onChanged: () => void | Promise<void>;
  } = $props();

  let url = $state('');
  let username = $state('');
  let password = $state('');
  let intervalHours = $state(24);
  let saving = $state(false);
  let syncing = $state(false);
  let deleting = $state(false);
  let error = $state('');
  let success = $state('');

  // Re-fill the form whenever the parent reloads a (possibly different) mirror
  $effect(() => {
    fillForm(mirror);
  });

  function fillForm(next: RepositoryMirror | null) {
    url = next?.url ?? '';
    username = next?.username ?? '';
    password = '';
    intervalHours = Math.max(1, Math.round((next?.sync_interval_seconds ?? 86400) / 3600));
  }

  function payload() {
    const trimmedUrl = url.trim();
    const trimmedUsername = username.trim();
    const trimmedPassword = password.trim();

    return {
      url: trimmedUrl,
      username: trimmedUsername || undefined,
      password: trimmedPassword || undefined,
      sync_interval_seconds: Math.max(1, Math.round(intervalHours)) * 3600,
    };
  }

  async function saveMirror(event: SubmitEvent) {
    event.preventDefault();
    if (!url.trim()) {
      error = t('settings.mirror.url_required');
      return;
    }

    try {
      saving = true;
      error = '';
      success = '';
      const wasConfigured = Boolean(mirror);
      const next = mirror
        ? await mirrors.update(owner, repo, payload())
        : await mirrors.create(owner, repo, payload());
      success = wasConfigured ? t('settings.mirror.updated') : t('settings.mirror.created');
      await onChanged();
      fillForm(next);
    } catch (err: any) {
      error = toErrorMessage(err, t('settings.mirror.save_failed'));
    } finally {
      saving = false;
    }
  }

  async function syncMirror() {
    try {
      syncing = true;
      error = '';
      success = '';
      await mirrors.sync(owner, repo);
      success = t('settings.mirror.sync_started');
      await onChanged();
    } catch (err: any) {
      error = toErrorMessage(err, t('settings.mirror.sync_failed'));
    } finally {
      syncing = false;
    }
  }

  async function removeMirror() {
    if (!mirror || !(await confirmDialog({ variant: 'danger', message: t('settings.mirror.delete_confirm') }))) return;

    try {
      deleting = true;
      error = '';
      success = '';
      await mirrors.remove(owner, repo);
      success = t('settings.mirror.deleted');
      await onChanged();
    } catch (err: any) {
      error = toErrorMessage(err, t('settings.mirror.delete_failed'));
    } finally {
      deleting = false;
    }
  }
</script>

<section class="section">
  <h2>{mirror ? t('settings.mirror.edit_title') : t('settings.mirror.create_title')}</h2>

  {#if success}
    <SuccessBanner message={success} />
  {/if}

  {#if error}
    <ErrorBanner message={error} />
  {/if}

  <form class="mirror-form" onsubmit={saveMirror}>
    <FormField id="mirror-url" label={t('settings.mirror.url')}>
      {#snippet children()}
        <input
          id="mirror-url"
          type="url"
          bind:value={url}
          placeholder="https://github.com/org/repo.git"
          disabled={saving}
        />
      {/snippet}
    </FormField>

    <div class="form-row">
      <FormField id="mirror-username" label={t('settings.mirror.username')}>
        {#snippet children()}
          <input
            id="mirror-username"
            type="text"
            bind:value={username}
            autocomplete="username"
            disabled={saving}
          />
        {/snippet}
      </FormField>
      <FormField id="mirror-password" label={t('settings.mirror.password')}>
        {#snippet children()}
          <input
            id="mirror-password"
            type="password"
            bind:value={password}
            autocomplete="new-password"
            disabled={saving}
            placeholder={mirror ? t('settings.mirror.password_placeholder') : ''}
          />
        {/snippet}
      </FormField>
    </div>

    <div class="form-group small">
      <label for="mirror-interval">{t('settings.mirror.interval_hours')}</label>
      <input
        id="mirror-interval"
        type="number"
        min="1"
        step="1"
        bind:value={intervalHours}
        disabled={saving}
      />
    </div>

    <div class="actions">
      <button class="btn btn-primary" type="submit" disabled={saving || !url.trim()}>
        {saving ? t('common.loading') : t('common.save')}
      </button>
      {#if mirror}
        <button
          class="btn btn-outline"
          type="button"
          onclick={syncMirror}
          disabled={syncing || saving || deleting}
        >
          {syncing ? t('common.loading') : t('settings.mirror.sync_now')}
        </button>
        <button
          class="btn btn-danger"
          type="button"
          onclick={removeMirror}
          disabled={deleting || saving || syncing}
        >
          {deleting ? t('common.loading') : t('settings.mirror.delete')}
        </button>
      {/if}
    </div>
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

  .mirror-form {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .form-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 1rem;
  }

  .form-group.small {
    max-width: 180px;
  }

  label {
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem;
  }

  @media (max-width: 720px) {
    .form-row {
      grid-template-columns: 1fr;
    }
  }
</style>
