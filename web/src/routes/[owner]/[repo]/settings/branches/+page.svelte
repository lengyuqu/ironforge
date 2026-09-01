<script lang="ts">
  // Branch protection settings page — orchestration layer: loads the rule
  // list and tracks which rule is being edited; create/edit/delete run in
  // the self-contained form/list components.
  import { page } from '$app/stores';
  import { branchProtections, type BranchProtectionRule } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import BranchProtectionForm from '$lib/components/settings/BranchProtectionForm.svelte';
  import BranchProtectionList from '$lib/components/settings/BranchProtectionList.svelte';

  const t = createT();
  const owner = $derived($page.params.owner!);
  const repo = $derived($page.params.repo!);

  let rules = $state<BranchProtectionRule[]>([]);
  let loading = $state(true);
  let error = $state('');
  let editingRule = $state<BranchProtectionRule | null>(null);

  $effect(() => {
    loadRules();
  });

  async function loadRules() {
    try {
      loading = true;
      error = '';
      rules = await branchProtections.list(owner, repo);
    } catch (e: unknown) {
      error = toErrorMessage(e, t('settings.branch_protection.load_failed', 'Load failed'));
    } finally {
      loading = false;
    }
  }

  // Re-key the form so it remounts with fresh defaults after a save.
  let formKey = $state(0);

  function handleSaved() {
    editingRule = null;
    formKey += 1;
    loadRules();
  }

  function handleEdit(rule: BranchProtectionRule) {
    editingRule = rule;
    formKey += 1;
  }

  function cancelEdit() {
    editingRule = null;
    formKey += 1;
  }

  function handleDeleted(ruleId: number) {
    if (editingRule?.id === ruleId) {
      editingRule = null;
      formKey += 1;
    }
  }
</script>

<div class="branch-protection-page">
  <div class="page-header">
    <div>
      <h1>{t('settings.branch_protection.title')}</h1>
      <p>{t('settings.branch_protection.desc')}</p>
    </div>
  </div>

  {#if error}
    <div class="error-box">{error}</div>
  {/if}

  {#key formKey}
    <BranchProtectionForm
      {owner}
      {repo}
      editingRule={editingRule}
      onSaved={handleSaved}
      onCancel={cancelEdit}
    />
  {/key}

  <BranchProtectionList
    {owner}
    {repo}
    {rules}
    {loading}
    editingId={editingRule?.id ?? null}
    onEdit={handleEdit}
    onDeleted={handleDeleted}
    onChanged={loadRules}
  />
</div>

<style>
  .branch-protection-page {
    max-width: 960px;
  }

  .page-header {
    margin-bottom: 2rem;
  }

  h1 {
    font-size: 1.75rem;
    margin: 0 0 0.5rem;
    color: var(--text-primary);
  }

  p {
    margin: 0;
    color: var(--text-secondary);
    font-size: 0.95rem;
  }

  .error-box {
    padding: 1rem;
    border-radius: 6px;
    margin-bottom: 1rem;
    background: var(--danger-bg, #fee2e2);
    color: var(--danger-text, #991b1b);
  }
</style>
