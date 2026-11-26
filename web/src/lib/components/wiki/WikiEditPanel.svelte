<script lang="ts">
  import { wiki } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { useUnsavedChangesGuard } from '$lib/utils/unsavedChanges.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    repo,
    title,
    initialContent,
    initialVersion,
    onSaved,
    onCancel,
  }: {
    owner: string;
    repo: string;
    title: string;
    initialContent: string;
    initialVersion: string;
    onSaved: () => void | Promise<void>;
    onCancel: () => void;
  } = $props();

  let editContent = $state(initialContent);
  let saving = $state(false);
  const dirty = $derived.by(() => editContent !== initialContent);
  const guard = useUnsavedChangesGuard(() => dirty);

  async function handleSave() {
    saving = true;
    try {
      await guard.suspend(async () => {
        await wiki.update(owner, repo, title, editContent, initialVersion);
        toast.success(t('wiki.save') || 'Saved');
        await onSaved();
      });
    } catch (e) {
      toast.error(toErrorMessage(e, t('errors.save_failed')));
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
