<script lang="ts">
  // Recent commits panel — pure presentation: lists the latest commits with
  // links to the commit detail view.
  import { buildCommitHref } from '$lib/utils/repoUrls';
  import { createT, formatDate } from '$lib/i18n';
  import type { RepoCommitEntry } from '$lib/types/entities';

  interface Props {
    owner: string;
    repo: string;
    ref: string;
    commits: RepoCommitEntry[];
  }

  let { owner, repo, ref, commits }: Props = $props();

  const t = createT();
</script>

<div class="gh-card commits-panel">
  <h3>{t('repo.browser.recent_commits')}</h3>
  {#each commits as commit (commit.sha)}
    <a href={buildCommitHref(owner, repo, ref, commit.sha)} class="commit-item">
      <div class="commit-msg truncate">{commit.message?.split('\n')[0]}</div>
      <div class="commit-meta">
        <span class="commit-author">{commit.author}</span>
        <span class="commit-date">{formatDate(commit.date)}</span>
        <code class="commit-sha">{commit.sha?.slice(0, 7)}</code>
      </div>
    </a>
  {/each}
</div>

<style>
  .commits-panel {
    padding: 16px;
  }

  h3 { font-size: 14px; margin-bottom: 12px; }

  .commit-item {
    display: block;
    padding: 8px 0;
    border-bottom: 1px solid var(--border-light);
    color: inherit;
    text-decoration: none;
  }
  .commit-item:last-child { border-bottom: none; }

  .commit-msg {
    font-size: 13px;
    font-weight: 500;
    margin-bottom: 4px;
  }

  .commit-meta {
    display: flex;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
    align-items: center;
  }

  .commit-sha {
    font-size: 11px;
    background: var(--bg-tertiary);
    padding: 1px 6px;
    border-radius: 4px;
    color: var(--accent);
  }
</style>
