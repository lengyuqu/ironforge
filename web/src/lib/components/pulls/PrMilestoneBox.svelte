<script lang="ts">
  // PR milestone box (A4) — shows the current milestone and lets repo writers
  // attach / clear it. Self-contained: loads the milestone list itself and
  // delegates persistence to the pulls.update endpoint (tri-state null clears).
  import { milestones, pulls } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import type { Milestone, PullRequest } from '$lib/types/entities';

  interface Props {
    owner: string;
    repo: string;
    pr: PullRequest;
    onChanged?: () => void;
  }

  let { owner, repo, pr, onChanged }: Props = $props();

  const t = createT();

  let milestoneOptions = $state<Milestone[]>([]);
  let updating = $state(false);

  $effect(() => {
    loadMilestones();
  });

  async function loadMilestones() {
    try {
      milestoneOptions = await milestones.list(owner, repo);
    } catch (e: unknown) {
      toast.error(toErrorMessage(e, t('errors.load_failed', 'Load failed')));
    }
  }

  async function handleChange(event: Event) {
    const value = (event.target as HTMLSelectElement).value;
    const next = value === '' ? null : Number(value);
    if (next === (pr.milestone_id ?? null)) return;
    try {
      updating = true;
      await pulls.update(owner, repo, pr.number, { milestone_id: next });
      toast.success(t('issues.milestone_saved', 'Milestone updated'));
      onChanged?.();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      updating = false;
    }
  }
</script>

<section class="milestone-box">
  <h3>{t('issues.milestone')}</h3>
  {#if updating}
    <p class="text-secondary">{t('common.loading')}</p>
  {:else}
    <select
      value={pr.milestone_id ? String(pr.milestone_id) : ''}
      onchange={handleChange}
      disabled={milestoneOptions.length === 0 && !pr.milestone_id}
    >
      <option value="">{t('issues.no_milestone', 'No milestone')}</option>
      {#each milestoneOptions as ms (ms.id)}
        <option value={ms.id} selected={pr.milestone_id === ms.id}>
          {ms.title}{ms.state === 'closed' ? ' · ' + t('issues.state_label.closed') : ''}
        </option>
      {/each}
    </select>
    {#if milestoneOptions.length === 0}
      <p class="text-secondary milestone-none">{t('settings.no_milestones')}</p>
    {/if}
  {/if}
</section>

<style>
  .milestone-box {
    padding: 12px 16px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-secondary);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .milestone-box h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .milestone-box select {
    width: 100%;
    padding: 6px 8px;
  }

  .milestone-none {
    font-size: 12px;
  }
</style>
