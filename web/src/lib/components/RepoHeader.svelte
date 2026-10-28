<script lang="ts">
  import RepoActions from '$lib/components/repo/RepoActions.svelte';
  import RepoTabs from '$lib/components/repo/RepoTabs.svelte';

  interface Props {
    owner: string;
    repo: string;
    activeTab?: string;
    starsCount?: number;
    defaultBranch?: string;
  }

  let { owner, repo, activeTab = 'code', starsCount = 0, defaultBranch }: Props = $props();
</script>

<div class="repo-header">
  <div class="repo-top">
    <div class="repo-name">
      <a href={`/${owner}`}>{owner}</a>
      <span class="separator">/</span>
      <a href={`/${owner}/${repo}`}>{repo}</a>
    </div>

    <RepoActions {owner} {repo} {starsCount} {defaultBranch} />
  </div>

  <RepoTabs {owner} {repo} {activeTab} />
</div>

<style>
  .repo-header {
    border-bottom: 1px solid var(--border);
    padding-bottom: 0;
    margin-bottom: 24px;
  }

  .repo-top {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    flex-wrap: wrap;
    gap: 12px;
  }

  .repo-name {
    font-size: 20px;
    margin-bottom: 12px;
  }
  .repo-name a {
    color: var(--accent);
    font-weight: 600;
  }
  .separator {
    margin: 0 4px;
    color: var(--text-muted);
  }

  @media (max-width: 600px) {
    .repo-top {
      flex-direction: column;
    }
  }
</style>
