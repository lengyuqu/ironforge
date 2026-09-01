<script lang="ts">
  import { boards } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';

  let {
    owner,
    repo,
    onCreated,
    onCancel,
  }: {
    owner: string;
    repo: string;
    onCreated: (boardId: number) => void | Promise<void>;
    onCancel: () => void;
  } = $props();

  let name = $state('');
  let creating = $state(false);

  async function handleCreate() {
    if (!name.trim() || creating) return;
    creating = true;
    try {
      const board = await boards.create(owner, repo, { name: name.trim() });
      name = '';
      await onCreated(board.id);
    } catch (e) {
      toast.error(toErrorMessage(e, 'Create board failed'));
    } finally {
      creating = false;
    }
  }
</script>

<div class="inline-form">
  <input
    class="form-input"
    placeholder="Board name"
    bind:value={name}
    onkeydown={(e) => e.key === 'Enter' && handleCreate()}
  />
  <button class="btn-primary btn-sm" onclick={handleCreate} disabled={creating}>
    {creating ? '…' : 'Create'}
  </button>
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
  .btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn-ghost {
    padding: 5px 10px; background: none; border: none;
    color: var(--text-secondary); font-size: 13px; cursor: pointer; border-radius: var(--radius);
  }
  .btn-ghost:hover { background: var(--bg-secondary); color: var(--text-primary); }
  .btn-sm { padding: 4px 10px; font-size: 12px; }
</style>
