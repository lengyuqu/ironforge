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
    width: 100%;
    resize: vertical;
    box-sizing: border-box;
  }
  button:disabled { opacity: 0.6; cursor: not-allowed; }
</style>
