<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import SuccessBanner from '$lib/components/SuccessBanner.svelte';
  import { confirmDialog } from '$lib/components/confirm.svelte';
  import { pushMirrors, type PushMirror } from '$lib/api/push-mirrors';
  import { createT } from '$lib/i18n';

  interface Props {
    owner: string;
    repo: string;
    onChanged?: () => void;
  }

  let { owner, repo, onChanged }: Props = $props();

  const t = createT();

  let pushMirror = $state<PushMirror | null>(null);
  let loading = $state(true);
  let saving = $state(false);
  let syncing = $state(false);
  let toggling = $state(false);
  let deleting = $state(false);
  let error = $state('');
  let success = $state('');

  let url = $state('');
  let pushTags = $state(true);
  let triggerMode = $state<'on_push' | 'manual'>('on_push');

  $effect(() => {
    load();
  });

  async function load() {
    try {
      loading = true;
      error = '';
      const existing = await pushMirrors.get(owner, repo);
      pushMirror = existing;
      url = existing.url;
      pushTags = existing.push_tags;
      triggerMode = existing.trigger_mode === 'manual' ? 'manual' : 'on_push';
    } catch (err) {
      // 404 is the normal "not configured yet" state, not an error.
      if ((err instanceof Error ? err.message : '').toLowerCase().includes('no push mirror')) {
        pushMirror = null;
      } else {
        error = toErrorMessage(err, t('settings.push_mirror.load_failed'));
      }
    } finally {
      loading = false;
    }
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (!url.trim()) {
      error = t('settings.push_mirror.url_required');
      return;
    }
    try {
      saving = true;
      error = '';
      success = '';
      pushMirror = await pushMirrors.create(owner, repo, {
        url: url.trim(),
        push_tags: pushTags,
        trigger_mode: triggerMode,
      });
      success = t('settings.push_mirror.saved');
      onChanged?.();
    } catch (err) {
      error = toErrorMessage(err, t('settings.push_mirror.save_failed'));
    } finally {
      saving = false;
    }
  }

  async function pushNow() {
    try {
      syncing = true;
      error = '';
      success = '';
      await pushMirrors.sync(owner, repo);
      success = t('settings.push_mirror.pushed');
    } catch (err) {
      error = toErrorMessage(err, t('settings.push_mirror.push_failed'));
    } finally {
      syncing = false;
      // Refresh either way: a failed push stores its reason on the row.
      await load();
      onChanged?.();
    }
  }

  async function toggleStatus() {
    if (!pushMirror) return;
    const next = pushMirror.status === 'active' ? 'paused' : 'active';
    try {
      toggling = true;
      error = '';
      success = '';
      pushMirror = await pushMirrors.update(owner, repo, { status: next });
      success =
        next === 'active'
          ? t('settings.push_mirror.resumed')
          : t('settings.push_mirror.paused');
      onChanged?.();
    } catch (err) {
      error = toErrorMessage(err, t('settings.push_mirror.update_failed'));
    } finally {
      toggling = false;
    }
  }

  async function remove() {
    if (!(await confirmDialog({ variant: 'danger', message: t('settings.push_mirror.delete_confirm') })))
      return;
    try {
      deleting = true;
      error = '';
      success = '';
      await pushMirrors.remove(owner, repo);
      pushMirror = null;
      url = '';
      success = t('settings.push_mirror.deleted');
      onChanged?.();
    } catch (err) {
      error = toErrorMessage(err, t('settings.push_mirror.delete_failed'));
    } finally {
      deleting = false;
    }
  }

  function formatDate(value?: string | null) {
    if (!value) return t('settings.push_mirror.never');
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
  }
</script>

<section class="push-mirror">
  <header>
    <h2>{t('settings.push_mirror.title')}</h2>
    <p>{t('settings.push_mirror.desc')}</p>
  </header>

  {#if error}<ErrorBanner message={error} />{/if}
  {#if success}<SuccessBanner message={success} />{/if}

  {#if loading}
    <p class="muted">{t('settings.push_mirror.loading')}</p>
  {:else}
    <form class="push-form" onsubmit={save}>
      <label for="push-url">{t('settings.push_mirror.url')}</label>
      <input
        id="push-url"
        class="text-input"
        bind:value={url}
        placeholder="https://github.com/acme/private.git"
        disabled={saving}
        spellcheck="false"
        autocomplete="off"
      />
      <p class="hint">{t('settings.push_mirror.url_hint')}</p>

      <label for="push-trigger">{t('settings.push_mirror.trigger')}</label>
      <select id="push-trigger" bind:value={triggerMode} disabled={saving}>
        <option value="on_push">{t('settings.push_mirror.trigger_on_push')}</option>
        <option value="manual">{t('settings.push_mirror.trigger_manual')}</option>
      </select>

      <label class="checkbox">
        <input type="checkbox" bind:checked={pushTags} disabled={saving} />
        {t('settings.push_mirror.push_tags')}
      </label>

      <div>
        <button class="btn btn-primary" type="submit" disabled={saving}>
          {saving ? t('settings.push_mirror.saving') : t('settings.push_mirror.save')}
        </button>
      </div>
    </form>

    {#if pushMirror}
      <article class="status-card">
        <div class="status-head">
          <h3>{t('settings.push_mirror.state')}</h3>
          <span class="badge {pushMirror.status}">{pushMirror.status}</span>
        </div>
        <code>{pushMirror.url}</code>
        <dl>
          <div>
            <dt>{t('settings.push_mirror.trigger')}</dt>
            <dd>{pushMirror.trigger_mode}</dd>
          </div>
          <div>
            <dt>{t('settings.push_mirror.last_push')}</dt>
            <dd>{formatDate(pushMirror.last_push_at)}</dd>
          </div>
        </dl>
        {#if pushMirror.last_push_error}
          <p class="error-text" role="alert">{pushMirror.last_push_error}</p>
        {/if}
        <div class="actions">
          <button class="btn" type="button" onclick={pushNow} disabled={syncing}>
            {syncing ? t('settings.push_mirror.pushing') : t('settings.push_mirror.push_now')}
          </button>
          <button class="btn" type="button" onclick={toggleStatus} disabled={toggling}>
            {pushMirror.status === 'active'
              ? t('settings.push_mirror.pause')
              : t('settings.push_mirror.resume')}
          </button>
          <button class="btn btn-danger" type="button" onclick={remove} disabled={deleting}>
            {deleting ? t('settings.push_mirror.deleting') : t('settings.push_mirror.delete')}
          </button>
        </div>
      </article>
    {/if}
  {/if}
</section>

<style>
  .push-mirror {
    margin-top: 2.5rem;
    padding-top: 2rem;
    border-top: 1px solid var(--border);
  }

  h2 {
    margin: 0 0 0.5rem;
    color: var(--text-primary);
  }

  h3 {
    margin: 0;
    font-size: 0.95rem;
  }

  p {
    margin: 0;
    color: var(--text-secondary);
    font-size: 0.95rem;
  }

  .push-form {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    margin-top: 1.25rem;
  }

  label {
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .hint {
    margin-top: -0.3rem;
    font-size: 0.8rem;
    line-height: 1.5;
  }

  .text-input,
  select {
    width: 100%;
  }

  .checkbox {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-weight: 400;
  }

  .checkbox input {
    width: auto;
  }

  .status-card {
    margin-top: 1.5rem;
    padding: 1rem;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .status-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }

  .badge {
    padding: 2px 10px;
    border: 1px solid var(--border);
    border-radius: 999px;
    font-size: 0.75rem;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .badge.active {
    color: var(--green);
    border-color: var(--green);
  }

  .badge.error {
    color: var(--red);
    border-color: var(--red);
  }

  .badge.paused {
    color: var(--text-secondary);
  }

  code {
    display: block;
    margin-top: 0.6rem;
    overflow-wrap: anywhere;
    color: var(--text-secondary);
  }

  dl {
    display: flex;
    flex-wrap: wrap;
    gap: 1.25rem;
    margin: 0.75rem 0 0;
  }

  dl div {
    display: flex;
    gap: 0.4rem;
  }

  dt {
    color: var(--text-secondary);
  }

  dd {
    margin: 0;
  }

  .error-text {
    margin-top: 0.75rem;
    padding: 0.6rem 0.75rem;
    color: var(--red);
    background: color-mix(in srgb, var(--red) 10%, transparent);
    border-radius: var(--radius);
    font-size: 0.85rem;
    overflow-wrap: anywhere;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-top: 1rem;
  }

  .muted {
    color: var(--text-secondary);
  }
</style>
