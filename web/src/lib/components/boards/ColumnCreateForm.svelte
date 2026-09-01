<script lang="ts">
  import { boards } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';

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
      toast.error(toErrorMessage(e, 'Add column failed'));
    }
  }
</script>

<div class="inline-form">
  <input
    class="form-input"
    placeholder="Column name"
    bind:value={name}
    onkeydown={(e) => e.key === 'Enter' && handleAdd()}
  />
  <button class="btn-primary btn-sm" onclick={handleAdd}>Add</button>
  <button class="btn-ghost btn-sm" onclick={onCancel}>Cancel</button>
</div>

<style>
  .inline-form {
    display: flex; gap: 8px; align-items: center;
    margin-bottom: 16px; flex-wrap: wrap;
  }
  .form-input {
    flex: 1; min-width: 180px; padding: 6px 12px;
    border: 1px solid var(--border); border-radius: var(--radius);
    background: var(--bg-primary); color: var(--text-primary); font-size: 14px;
  }
  .btn-primary {
    padding: 6px 14px; background: var(--accent); color: #fff; border: none;
    border-radius: var(--radius); font-size: 13px; font-weight: 600; cursor: pointer;
  }
  .btn-ghost {
    padding: 5px 10px; background: none; border: none;
    color: var(--text-secondary); font-size: 13px; cursor: pointer; border-radius: var(--radius);
  }
  .btn-ghost:hover { background: var(--bg-secondary); color: var(--text-primary); }
  .btn-sm { padding: 4px 10px; font-size: 12px; }
</style>
