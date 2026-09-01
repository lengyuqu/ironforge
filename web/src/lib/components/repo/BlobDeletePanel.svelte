<script lang="ts">
  // Blob delete panel — inline confirmation form under the file header.
  // Self-contained: performs the delete via the contents API, keeps conflict
  // / failure errors inline, and lets the page navigate on success.
  import { repos } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';

  interface Props {
    owner: string;
    repo: string;
    ref: string;
    filePath: string;
    sha: string;
    onClose: () => void;
    onDeleted: () => void;
  }

  let { owner, repo, ref, filePath, sha, onClose, onDeleted }: Props = $props();

  const t = createT();

  let deleteMessage = $state(`Delete ${filePath}`);
  let deleteError = $state('');
  let deleting = $state(false);

  function isConflictError(message: string): boolean {
    const normalized = message.toLowerCase();
    return normalized.includes('sha mismatch') || normalized.includes('conflict');
  }

  async function deleteFile() {
    deleting = true;
    deleteError = '';
    try {
      await repos.deleteContent(owner, repo, filePath, {
        branch: ref || undefined,
        message: deleteMessage.trim() || `Delete ${filePath}`,
        sha
      });
      onDeleted();
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      deleteError = isConflictError(message)
        ? t('repo.blob.delete_conflict')
        : t('repo.blob.delete_failed', { message });
    } finally {
      deleting = false;
    }
  }
</script>

<div class="delete-panel">
  <div>
    <strong>{t('repo.blob.delete_title')}</strong>
    <p>{filePath}</p>
  </div>
  {#if deleteError}
    <div class="delete-error">{deleteError}</div>
  {/if}
  <label for="delete-message">{t('repo.blob.delete_message')}</label>
  <input
    id="delete-message"
    bind:value={deleteMessage}
    placeholder={t('repo.blob.delete_placeholder', { path: filePath })}
  />
  <div class="delete-actions">
    <button type="button" class="btn-outline btn-sm" onclick={onClose}>
      {t('common.cancel')}
    </button>
    <button type="button" class="btn-danger btn-sm" onclick={deleteFile} disabled={deleting}>
      {deleting ? t('repo.blob.deleting') : t('repo.blob.delete_file')}
    </button>
  </div>
</div>

<style>
  .delete-panel {
    display: grid;
    gap: 10px;
    border: 1px solid color-mix(in srgb, #cf222e 30%, transparent);
    border-bottom: none;
    background: color-mix(in srgb, #cf222e 7%, var(--bg-primary));
    padding: 10px 16px;
    font-size: 13px;
  }

  .delete-panel p {
    margin: 3px 0 0;
    color: var(--text-muted);
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
  }

  .delete-panel label {
    font-size: 13px;
    font-weight: 600;
  }

  .delete-panel input {
    min-height: 36px;
    padding: 7px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-primary);
    color: var(--text-primary);
  }

  .delete-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .btn-outline {
    border-radius: 6px;
    min-height: 32px;
    padding: 5px 10px;
    font-size: 13px;
    text-decoration: none;
    cursor: pointer;
    border: 1px solid var(--border);
    background: var(--bg-primary);
    color: var(--text-primary);
  }

  .btn-outline:hover {
    background: var(--bg-tertiary);
  }

  .btn-danger {
    border-radius: 6px;
    min-height: 32px;
    padding: 5px 10px;
    font-size: 13px;
    text-decoration: none;
    cursor: pointer;
    border: 1px solid #cf222e;
    background: #cf222e;
    color: white;
  }

  .btn-danger:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .delete-error {
    color: #cf222e;
    font-size: 13px;
  }

  @media (max-width: 820px) {
    .delete-actions {
      justify-content: flex-start;
    }
  }
</style>
