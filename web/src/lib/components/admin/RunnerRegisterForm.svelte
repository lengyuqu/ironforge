<script lang="ts">
  // Runner registration form — self-contained: registers via the runners
  // API and shows the one-time token banner (with copy) inline after
  // success; the parent only needs to reload the list via onRegistered.
  import { runners } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';

  interface Props {
    onRegistered: () => void | Promise<void>;
  }

  let { onRegistered }: Props = $props();

  const t = createT();

  let newRunnerName = $state('');
  let newRunnerLabels = $state('');
  let saving = $state(false);
  let formError = $state('');
  let registeredRunner = $state<{ id: number; token: string; name: string } | null>(null);

  async function handleRegister() {
    if (!newRunnerName.trim() || saving) return;
    try {
      saving = true;
      formError = '';
      const labels = newRunnerLabels
        ? newRunnerLabels.split(',').map((label) => label.trim()).filter(Boolean)
        : undefined;
      const response = await runners.register({
        name: newRunnerName.trim(),
        labels,
      });
      registeredRunner = { id: response.id, token: response.token, name: newRunnerName.trim() };
      newRunnerName = '';
      newRunnerLabels = '';
      await onRegistered();
    } catch (e: unknown) {
      formError = toErrorMessage(e, t('errors.save_failed', 'Save failed'));
    } finally {
      saving = false;
    }
  }

  async function copyRunnerToken() {
    if (!registeredRunner) return;
    await navigator.clipboard.writeText(registeredRunner.token);
  }
</script>

{#if registeredRunner}
  <div class="token-banner">
    <div>
      <strong>{t('admin.runners.token_title', { name: registeredRunner.name })}</strong>
      <p>{t('admin.runners.token_help')}</p>
      <code>{registeredRunner.token}</code>
    </div>
    <button class="btn-secondary" type="button" onclick={copyRunnerToken}>
      {t('common.copy')}
    </button>
  </div>
{/if}

<section class="panel">
  <h2>{t('admin.runners.register')}</h2>
  {#if formError}
    <div class="error">{formError}</div>
  {/if}
  <div class="form-grid">
    <label>
      <span>{t('admin.runners.name')}</span>
      <input type="text" bind:value={newRunnerName} placeholder="linux-runner-01" />
    </label>
    <label>
      <span>{t('admin.runners.labels')}</span>
      <input type="text" bind:value={newRunnerLabels} placeholder="linux,x86_64,docker" />
    </label>
    <button class="btn-primary" onclick={handleRegister} disabled={saving || !newRunnerName.trim()}>
      {saving ? t('common.loading') : t('admin.runners.register')}
    </button>
  </div>
</section>

<style>
  .panel,
  .token-banner {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1rem;
    margin-bottom: 1rem;
  }

  .token-banner {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 1rem;
  }

  .token-banner p {
    color: var(--text-secondary);
    margin: 0.25rem 0 0.75rem;
  }

  .token-banner code {
    display: block;
    max-width: 100%;
    overflow-x: auto;
    padding: 0.5rem;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .form-grid {
    display: grid;
    grid-template-columns: minmax(160px, 1fr) minmax(220px, 1.5fr) auto;
    gap: 0.75rem;
    align-items: end;
  }

  h2 {
    margin: 0 0 1rem;
    font-size: 1rem;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  label span {
    color: var(--text-secondary);
    font-size: 0.85rem;
    font-weight: 600;
  }

  input {
    padding: 0.5rem 0.65rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-primary);
    color: var(--text-primary);
  }

  .error {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 0.5rem 0.75rem;
    border-radius: 6px;
    margin-bottom: 1rem;
  }

  .btn-secondary {
    background: var(--bg-primary);
    border: 1px solid var(--border);
    color: var(--text-primary);
    border-radius: 6px;
    padding: 0.45rem 0.8rem;
    cursor: pointer;
  }

  .btn-primary {
    background: var(--accent);
    border: 1px solid var(--accent);
    color: #fff;
    border-radius: 6px;
    padding: 0.5rem 0.9rem;
    cursor: pointer;
    font-weight: 600;
  }

  .btn-primary:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  @media (max-width: 720px) {
    .form-grid {
      grid-template-columns: 1fr;
    }

    .token-banner {
      flex-direction: column;
    }
  }
</style>
