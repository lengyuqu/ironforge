<script lang="ts">
  import { wiki } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    repo,
    title,
    initialContent,
    onSaved,
    onCancel,
  }: {
    owner: string;
    repo: string;
    title: string;
    initialContent: string;
    onSaved: () => void | Promise<void>;
    onCancel: () => void;
  } = $props();

  let editContent = $state(initialContent);
  let saving = $state(false);

  async function handleSave() {
    saving = true;
    try {
      await wiki.update(owner, repo, title, editContent);
      toast.success(t('wiki.save') || 'Saved');
      await onSaved();
    } catch (e) {
      toast.error(toErrorMessage(e, t('errors.save_failed') || 'Save failed'));
    } finally {
      saving = false;
    }
  }
</script>

<div class="edit-area">
  <textarea bind:value={editContent} rows="20" disabled={saving}></textarea>
  <div class="form-actions">
    <button class="btn-primary" onclick={handleSave} disabled={saving}>{t('wiki.save')}</button>
    <button class="btn-secondary" onclick={onCancel} disabled={saving}>{t('wiki.cancel')}</button>
  </div>
</div>

<style>
  .edit-area { margin-top: 8px; }
  textarea {
    width: 100%; padding: 12px; border: 1px solid var(--border);
    border-radius: var(--radius); font-size: 13px; font-family: var(--font-mono);
    background: var(--bg-primary); color: var(--text-primary);
    resize: vertical; box-sizing: border-box;
  }
  .form-actions { display: flex; gap: 8px; margin-top: 12px; }

  .btn-primary {
    padding: 6px 16px; background: var(--accent); color: #fff;
    border: none; border-radius: var(--radius); font-size: 13px; cursor: pointer;
  }
  .btn-secondary {
    padding: 6px 16px; background: var(--bg-tertiary); color: var(--text-primary);
    border: 1px solid var(--border); border-radius: var(--radius); font-size: 13px; cursor: pointer;
  }
  button:disabled { opacity: 0.6; cursor: not-allowed; }
</style>
