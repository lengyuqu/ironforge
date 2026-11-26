<script lang="ts">
  // A read-only PREVIEW of what deleting the repository would do to the
  // collaboration records. It deletes nothing and authorizes nothing, and the
  // counts are a point-in-time read: re-read before acting on them. Only
  // repository admins may ask; everyone else gets an explicit "not available".
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { ApiError } from '$lib/api/_base.svelte';
  import Button from '$lib/components/Button.svelte';
  import SectionCard from './SectionCard.svelte';
  import { collabOps, type RepoImpact } from '$lib/api/collabOps';
  import { formatTime } from '$lib/utils/collaboration';

  interface Props {
    owner: string;
    repo: string;
    /**
     * What this endpoint's answer says about the viewer: true = repository admin,
     * false = refused, null = not known yet. Only a read of this preview can say
     * it, so the settings page uses it to explain the admin-only sections.
     */
    admin?: boolean | null;
  }
  let { owner, repo, admin = $bindable(null) }: Props = $props();
  const t = createT();

  const COUNT_KEYS = ['sessions', 'tasks', 'messages', 'handoffs', 'takeovers', 'questions', 'answers', 'work_packages'] as const;

  let impact = $state<RepoImpact | null>(null);
  let loading = $state(true);
  let error = $state('');
  let forbidden = $state(false);

  async function load() {
    loading = true;
    error = '';
    forbidden = false;
    try {
      impact = await collabOps.deletionImpact(owner, repo);
      admin = true;
    } catch (e) {
      impact = null;
      if (e instanceof ApiError && (e.status === 403 || e.status === 401)) {
        forbidden = true;
        admin = false;
      } else {
        error = toErrorMessage(e, t('common.unknown_error'));
      }
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void owner;
    void repo;
    void load();
  });

  const known = (prefix: string, v: string, list: readonly string[]) => (list.includes(v) ? t(`${prefix}.${v}`) : v);
</script>

<SectionCard title={t('collab_ops.impact.title')} note={t('collab_ops.impact.note')} {loading} {error}>
  {#snippet actions()}
    <Button size="sm" onclick={load} disabled={loading}>{t('collab_ops.impact.refresh')}</Button>
  {/snippet}
  {#if forbidden}
    <p class="muted" data-testid="impact-forbidden">{t('collab_ops.impact.admin_only')}</p>
  {:else if impact}
    <p class="preview" role="note" data-testid="impact-preview">{t('collab_ops.impact.preview_banner')}</p>
    <table data-testid="impact-counts">
      <thead>
        <tr><th>{t('collab_ops.impact.record')}</th><th class="num">{t('collab_ops.impact.count')}</th></tr>
      </thead>
      <tbody>
        {#each COUNT_KEYS as k}
          <tr><td>{t(`collab_ops.impact.kind.${k}`)}</td><td class="num">{impact.records[k]}</td></tr>
        {/each}
      </tbody>
    </table>
    <dl class="kv">
      <dt>{t('collab_ops.impact.on_delete')}</dt>
      <dd>{known('collab_ops.impact.on_delete_value', impact.on_repository_delete, ['kept'])}</dd>
      <dt>{t('collab_ops.impact.on_row_removal')}</dt>
      <dd>{known('collab_ops.impact.on_row_removal_value', impact.on_repository_row_removal, ['cascade'])}</dd>
      <dt>{t('collab_ops.impact.git_retention')}</dt>
      <dd>{t('collab_ops.impact.days', { n: impact.git_data_retention_days })}</dd>
      <dt>{t('collab_ops.impact.retention_policy')}</dt>
      <dd>{known('collab_ops.impact.policy_value', impact.retention_policy, ['none'])}</dd>
    </dl>
    <p class="muted">{t('collab_ops.impact.as_of', { time: formatTime(impact.as_of) })}</p>
  {/if}
</SectionCard>

<style>
  .muted { color: var(--text-muted); font-size: 12px; margin: 6px 0; }
  .preview { margin: 0 0 10px; padding: 8px 12px; border-left: 3px solid var(--yellow); background: var(--bg-secondary); font-size: 12px; }
  table { border-collapse: collapse; font-size: 13px; margin-bottom: 12px; min-width: 240px; }
  th { text-align: left; color: var(--text-secondary); padding: 4px 12px 4px 0; border-bottom: 1px solid var(--border); }
  td { padding: 4px 12px 4px 0; border-bottom: 1px solid var(--border-light); }
  .num { text-align: right; font-variant-numeric: tabular-nums; }
  .kv { display: grid; grid-template-columns: fit-content(55%) minmax(0, 1fr); overflow-wrap: anywhere; gap: 4px 16px; margin: 0; font-size: 13px; }
  dt { color: var(--text-secondary); font-weight: 600; }
  dd { margin: 0; }
</style>
