<script lang="ts">
  // Branch protection rules table — renders the current rules; deletion is
  // self-contained (confirm + API + toast), editing delegates via onEdit.
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { confirmDialog } from '$lib/components/confirm.svelte';
  import { branchProtections, type BranchProtectionRule } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';

  interface Props {
    owner: string;
    repo: string;
    rules: BranchProtectionRule[];
    loading: boolean;
    /** Rule currently being edited (disables its row buttons). */
    editingId: number | null;
    onEdit: (rule: BranchProtectionRule) => void;
    /** Called after a successful delete; the page may reset the form. */
    onDeleted: (ruleId: number) => void;
    onChanged: () => void;
  }

  let { owner, repo, rules, loading, editingId, onEdit, onDeleted, onChanged }: Props = $props();

  const t = createT();

  let busyId = $state<number | null>(null);

  async function deleteRule(rule: BranchProtectionRule) {
    if (!(await confirmDialog({ variant: 'danger', message: t('settings.branch_protection.delete_confirm', { branch: rule.branch_name }) }))) return;

    try {
      busyId = rule.id;
      await branchProtections.remove(owner, repo, rule.id);
      toast.success(t('settings.branch_protection.deleted', 'Protection rule deleted'));
      onDeleted(rule.id);
      await onChanged();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e, t('settings.branch_protection.delete_failed', 'Delete failed')));
    } finally {
      busyId = null;
    }
  }
</script>

<section class="settings-section">
  <h2>{t('settings.branch_protection.current')}</h2>

  {#if loading}
    <LoadingState />
  {:else if rules.length === 0}
    <EmptyState title={t('settings.branch_protection.empty')} />
  {:else}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>{t('settings.branch_protection.branch')}</th>
            <th>{t('settings.branch_protection.rules')}</th>
            <th>{t('settings.branch_protection.updated_at')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each rules as rule (rule.id)}
            <tr>
              <td><code>{rule.branch_name}</code></td>
              <td>
                <div class="rule-list">
                  {#if rule.require_pr}<span>{t('settings.branch_protection.require_pr')}</span>{/if}
                  {#if rule.require_approval}<span>{t('settings.branch_protection.approvals_count', { count: rule.required_approvals || 1 })}</span>{/if}
                  {#if rule.require_status_check}<span>{t('settings.branch_protection.status_checks_enabled')}</span>{/if}
                  {#if rule.allow_force_push}<span>{t('settings.branch_protection.force_push_allowed')}</span>{/if}
                  {#if rule.require_signed_commits}<span>{t('settings.branch_protection.signed_commits_required', 'Signed commits required')}</span>{/if}
                </div>
              </td>
              <td>{new Date(rule.updated_at).toLocaleDateString()}</td>
              <td class="actions">
                <button class="btn btn-outline" onclick={() => onEdit(rule)} disabled={busyId === rule.id}>
                  {t('common.edit')}
                </button>
                <button class="btn btn-danger" onclick={() => deleteRule(rule)} disabled={busyId === rule.id}>
                  {t('common.delete')}
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<style>
  h2 {
    margin: 0 0 1rem;
    color: var(--text-primary);
  }

  .table-wrap {
    overflow-x: auto;
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  table {
    width: 100%;
    border-collapse: collapse;
  }

  th,
  td {
    padding: 0.8rem;
    border-bottom: 1px solid var(--border);
    text-align: left;
    vertical-align: top;
  }

  th {
    background: var(--bg-secondary);
    font-size: 0.8rem;
    color: var(--text-secondary);
    text-transform: uppercase;
  }

  tr:last-child td {
    border-bottom: 0;
  }

  code {
    font-family: var(--font-mono);
    font-size: 0.9rem;
  }

  .rule-list {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }

  .rule-list span {
    padding: 0.2rem 0.5rem;
    border-radius: 999px;
    background: var(--bg-secondary);
    color: var(--text-secondary);
    font-size: 0.8rem;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.75rem;
  }

  @media (max-width: 720px) {
    .actions {
      flex-direction: column;
    }
  }
</style>
