<script lang="ts">
  import { boards } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    repo,
    boardId,
    onCreated,
    onCancel,
  }: {
    owner: string;
    repo: string;
    boardId: number;
    onCreated: () => void | Promise<void>;
    onCancel: () => void;
  } = $props();

  let name = $state('');

  async function handleAdd() {
    if (!name.trim()) return;
    try {
      await boards.createColumn(owner, repo, boardId, { name: name.trim() });
      name = '';
      await onCreated();
    } catch (e) {
      toast.error(toErrorMessage(e, t('board.addColumnFailed')));
    }
  }
</script>

<div class="inline-form">
  <input
    class="form-input"
    placeholder={t('board.colNamePlaceholder')}
    bind:value={name}
    onkeydown={(e) => e.key === 'Enter' && handleAdd()}
  />
  <button class="btn-primary btn-sm" onclick={handleAdd}>{t('common.add')}</button>
  <button class="btn-ghost btn-sm" onclick={onCancel}>{t('common.cancel')}</button>
</div>

<style>
  .inline-form {
    display: flex; gap: 8px; align-items: center;
    margin-bottom: 16px; flex-wrap: wrap;
  }
  .form-input {
    flex: 1;
    min-width: 180px;
  }
</style>
