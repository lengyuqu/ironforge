<script lang="ts">
  // Commit status page — orchestration layer: loads the combined status,
  // status list, commit info (via log lookup) and GPG signature.
  import { page } from '$app/stores';
  import { repos } from '$lib/api/client.svelte';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import type { CombinedCommitStatus, CommitStatus, RepoCommitEntry } from '$lib/types/entities';
  import CommitInfoCard from '$lib/components/repo/CommitInfoCard.svelte';
  import StatusChecksPanel from '$lib/components/repo/StatusChecksPanel.svelte';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let sha = $derived($page.params.sha!);

  let loading = $state(true);
  let error = $state('');
  let commitInfo = $state<RepoCommitEntry | null>(null);
  let combinedStatus = $state<CombinedCommitStatus | null>(null);
  let statuses = $state<CommitStatus[]>([]);
  let gpgSignature = $state<Awaited<ReturnType<typeof repos.commitSignature>> | null>(null);

  $effect(() => {
    if (owner && repo && sha) {
      loadData();
    }
  });

  async function loadData() {
    loading = true;
    error = '';

    try {
      const [combinedResult, statusesResult] = await Promise.all([
        repos.getCombinedStatus(owner, repo, sha),
        repos.listCommitStatuses(owner, repo, sha)
      ]);

      combinedStatus = combinedResult;
      statuses = statusesResult;

      // Try to get commit info from log
      try {
        const logResult = await repos.log(owner, repo, sha);
        if (logResult.commits && logResult.commits.length > 0) {
          const commit = logResult.commits.find(
            (c) => c.sha.startsWith(sha) || sha.startsWith(c.sha)
          );
          if (commit) {
            commitInfo = commit;
          } else {
            // Use the first commit if exact match not found
            commitInfo = logResult.commits[0];
          }
        }
      } catch (logErr) {
        // Log endpoint might not support querying by sha, that's okay
        console.warn('Could not fetch commit info from log:', logErr);
      }

      // Fetch GPG signature
      try {
        gpgSignature = await repos.commitSignature(owner, repo, sha);
      } catch {
        /* GPG not available */
      }

      // If we couldn't get commit info, create a minimal version from sha
      if (!commitInfo) {
        commitInfo = {
          sha,
          message: sha,
          author: 'Unknown',
          date: new Date().toISOString()
        };
      }
    } catch (e: unknown) {
      error = toErrorMessage(e, t('errors.load_failed', 'Load failed'));
      console.error('Error loading commit status:', e);
    } finally {
      loading = false;
    }
  }
</script>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="commits" />

  <div class="commit-status-page">
    {#if loading}
      <div class="loading-container">
        <div class="spinner"></div>
        <p>Loading commit status...</p>
      </div>
    {:else if error}
      <div class="error-container">
        <p class="error-message">Error: {error}</p>
        <button onclick={() => loadData()}>Retry</button>
      </div>
    {:else if commitInfo}
      <CommitInfoCard commit={commitInfo} gpgSignature={gpgSignature} />
      <StatusChecksPanel combined={combinedStatus} {statuses} />
    {/if}
  </div>
</div>

<style>
  .commit-status-page {
    color: var(--text-primary);
  }

  .loading-container {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-height: 400px;
    gap: 1rem;
    color: var(--text-secondary);
  }

  .spinner {
    width: 40px;
    height: 40px;
    border: 3px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .error-container {
    text-align: center;
    padding: 3rem;
    color: var(--text-secondary);
  }

  .error-message {
    color: var(--red);
    margin-bottom: 1rem;
  }
</style>
