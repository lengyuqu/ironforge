<script lang="ts">
  // Branch protection form — self-contained: create/edit a protection rule
  // via the branchProtections API, reporting success/failure via toast.
  import {
    branchProtections,
    type BranchProtectionPayload,
    type BranchProtectionRule
  } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';

  interface Props {
    owner: string;
    repo: string;
    /** Rule being edited, or null when creating. */
    editingRule: BranchProtectionRule | null;
    onSaved: () => void;
    onCancel: () => void;
  }

  let { owner, repo, editingRule, onSaved, onCancel }: Props = $props();

  const t = createT();

  function parseStringList(value: string): string[] | undefined {
    const items = value
      .split(',')
      .map((item) => item.trim())
      .filter(Boolean);
    return items.length > 0 ? items : undefined;
  }

  function parseNumberList(value: string): number[] | undefined {
    const ids = value
      .split(',')
      .map((item) => Number(item.trim()))
      .filter((item) => Number.isInteger(item) && item > 0);
    return ids.length > 0 ? ids : undefined;
  }

  function parseJsonArray(value: string | null): string {
    if (!value) return '';
    try {
      const parsed = JSON.parse(value);
      return Array.isArray(parsed) ? parsed.join(', ') : '';
    } catch {
      return '';
    }
  }

  function emptyForm() {
    return {
      branch_name: 'main',
      require_pr: true,
      require_status_check: false,
      required_status_checks: '',
      require_approval: true,
      required_approvals: 1,
      allow_force_push: false,
      require_signed_commits: false,
      allowed_push_user_ids: ''
    };
  }

  // Snapshot the rule being edited (component remounts per edit switch).
  const initial = editingRule
    ? {
        branch_name: editingRule.branch_name,
        require_pr: editingRule.require_pr,
        require_status_check: editingRule.require_status_check,
        required_status_checks: parseJsonArray(editingRule.required_status_checks),
        require_approval: editingRule.require_approval,
        required_approvals: editingRule.required_approvals || 1,
        allow_force_push: editingRule.allow_force_push,
        require_signed_commits: editingRule.require_signed_commits,
        allowed_push_user_ids: parseJsonArray(editingRule.allowed_push_user_ids)
      }
    : emptyForm();

  let form = $state(initial);
  let saving = $state(false);

  function payload(includeBranch: boolean): BranchProtectionPayload {
    return {
      ...(includeBranch ? { branch_name: form.branch_name.trim() } : {}),
      require_pr: form.require_pr,
      require_status_check: form.require_status_check,
      required_status_checks: parseStringList(form.required_status_checks),
      require_approval: form.require_approval,
      required_approvals: form.require_approval ? Number(form.required_approvals || 1) : undefined,
      allow_force_push: form.allow_force_push,
      require_signed_commits: form.require_signed_commits,
      allowed_push_user_ids: parseNumberList(form.allowed_push_user_ids)
    };
  }

  async function saveRule(event: SubmitEvent) {
    event.preventDefault();

    if (!form.branch_name.trim()) {
      toast.error(t('settings.branch_protection.branch_required', 'Branch name is required'));
      return;
    }

    try {
      saving = true;
      if (editingRule) {
        await branchProtections.update(owner, repo, editingRule.id, payload(false));
        toast.success(t('settings.branch_protection.updated', 'Protection rule updated'));
      } else {
        await branchProtections.create(
          owner,
          repo,
          payload(true) as BranchProtectionPayload & { branch_name: string }
        );
        toast.success(t('settings.branch_protection.created', 'Protection rule created'));
      }
      onSaved();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e, t('settings.branch_protection.save_failed', 'Save failed')));
    } finally {
      saving = false;
    }
  }
</script>

<section class="section">
  <h2>{editingRule ? t('settings.branch_protection.edit_title') : t('settings.branch_protection.create_title')}</h2>
  <form class="rule-form" onsubmit={saveRule}>
    <div class="form-grid">
      <div class="form-group">
        <label for="protected-branch">{t('settings.branch_protection.branch')}</label>
        <input id="protected-branch" bind:value={form.branch_name} disabled={saving || Boolean(editingRule)} placeholder="main" />
      </div>

      <div class="form-group">
        <label for="required-approvals">{t('settings.branch_protection.required_approvals')}</label>
        <input id="required-approvals" type="number" min="1" bind:value={form.required_approvals} disabled={saving || !form.require_approval} />
      </div>

      <label class="check-row">
        <input type="checkbox" bind:checked={form.require_pr} disabled={saving} />
        <span>{t('settings.branch_protection.require_pr')}</span>
      </label>

      <label class="check-row">
        <input type="checkbox" bind:checked={form.require_approval} disabled={saving} />
        <span>{t('settings.branch_protection.require_approval')}</span>
      </label>

      <label class="check-row">
        <input type="checkbox" bind:checked={form.require_status_check} disabled={saving} />
        <span>{t('settings.branch_protection.require_status_check')}</span>
      </label>

      <label class="check-row">
        <input type="checkbox" bind:checked={form.allow_force_push} disabled={saving} />
        <span>{t('settings.branch_protection.allow_force_push')}</span>
      </label>

      <label class="check-row">
        <input type="checkbox" bind:checked={form.require_signed_commits} disabled={saving} />
        <span>{t('settings.branch_protection.require_signed_commits', 'Require cryptographically signed commits')}</span>
      </label>
    </div>

    <div class="form-group">
      <label for="required-checks">{t('settings.branch_protection.required_checks')}</label>
      <input id="required-checks" bind:value={form.required_status_checks} disabled={saving || !form.require_status_check} placeholder="test, lint" />
    </div>

    <div class="form-group">
      <label for="allowed-pushers">{t('settings.branch_protection.allowed_pushers')}</label>
      <input id="allowed-pushers" bind:value={form.allowed_push_user_ids} disabled={saving} placeholder="42, 108" />
    </div>

    <div class="form-actions">
      {#if editingRule}
        <button class="btn btn-outline" type="button" onclick={onCancel} disabled={saving}>
          {t('common.cancel')}
        </button>
      {/if}
      <button class="btn btn-primary" type="submit" disabled={saving}>
        {saving ? t('common.loading') : t('common.save')}
      </button>
    </div>
  </form>
</section>

<style>
  h2 {
    font-size: 1.1rem;
    margin: 0 0 1rem;
    color: var(--text-primary);
  }

  .section {
    margin-bottom: 2.5rem;
    padding-bottom: 2rem;
    border-bottom: 1px solid var(--border);
  }

  .rule-form {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .form-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 1rem;
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  label {
    font-size: 0.9rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  input {
    padding: 0.65rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-primary);
    color: var(--text-primary);
    font-size: 0.95rem;
  }

  .check-row {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    min-height: 42px;
  }

  .check-row input {
    width: 16px;
    height: 16px;
  }

  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.75rem;
  }

  .btn {
    padding: 0.55rem 0.9rem;
    border-radius: 6px;
    border: 1px solid var(--border);
    cursor: pointer;
  }

  .btn-primary {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
  }

  .btn-outline {
    background: transparent;
    color: var(--text-primary);
  }

  @media (max-width: 720px) {
    .form-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
