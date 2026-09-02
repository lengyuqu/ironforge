<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import AttachmentPanel from '$lib/components/AttachmentPanel.svelte';
  import { issues, type ReactionSummary } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import type { Issue, IssueComment } from '$lib/types/entities';
  import CommentCard from '$lib/components/issues/CommentCard.svelte';
  import AssigneesPanel from '$lib/components/issues/AssigneesPanel.svelte';
  import IssueMilestonePanel from '$lib/components/issues/IssueMilestonePanel.svelte';
  import IssueHeader from '$lib/components/issues/IssueHeader.svelte';
  import IssueCommentForm from '$lib/components/issues/IssueCommentForm.svelte';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let number = $derived(Number($page.params.number));

  let issue = $state<Issue | null>(null);
  let commentList = $state<IssueComment[]>([]);
  let loading = $state(true);
  let error = $state('');
  let issueReactions = $state<ReactionSummary[]>([]);
  let commentReactions = $state<Record<number, ReactionSummary[]>>({});

  $effect(() => {
    loadIssue();
  });

  async function loadIssue() {
    try {
      loading = true;
      const [issueData, commentsData, reactionsData] = await Promise.all([
        issues.get(owner, repo, number),
        issues.comments(owner, repo, number),
        issues.listReactions(owner, repo, number).catch(() => [] as ReactionSummary[]),
      ]);
      issue = issueData;
      commentList = commentsData || [];
      issueReactions = reactionsData || [];

      const commentIds = (commentsData || []).map((c) => c.id);
      const reactionEntries = await Promise.all(
        commentIds.map(async (id) => {
          const rows = await issues
            .listCommentReactions(owner, repo, id)
            .catch(() => [] as ReactionSummary[]);
          return [id, rows || []] as const;
        }),
      );
      commentReactions = Object.fromEntries(reactionEntries);
    } catch (e: unknown) {
      error = toErrorMessage(e, t('errors.load_failed', 'Load failed'));
    } finally {
      loading = false;
    }
  }

  async function toggleIssueReaction(content: string) {
    try {
      const mine = issueReactions.find((r) => r.content === content && r.reacted_by_me);
      issueReactions = mine
        ? await issues.removeReaction(owner, repo, number, content)
        : await issues.addReaction(owner, repo, number, content);
    } catch (e: unknown) {
      error = toErrorMessage(e);
    }
  }

  async function toggleCommentReaction(commentId: number, content: string) {
    try {
      const rows = commentReactions[commentId] || [];
      const mine = rows.find((r) => r.content === content && r.reacted_by_me);
      commentReactions[commentId] = mine
        ? await issues.removeCommentReaction(owner, repo, commentId, content)
        : await issues.addCommentReaction(owner, repo, commentId, content);
    } catch (e: unknown) {
      error = toErrorMessage(e);
    }
  }
</script>

<svelte:head>
  <title>{issue?.title || `${t('issues.title')} #${number}`} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="issues" starsCount={0} />

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">{t('common.loading')}</p>
  {:else if issue}
    <div class="issue-detail">
      <IssueHeader {issue} />

      <AssigneesPanel {owner} {repo} issueNumber={number} />

      <IssueMilestonePanel
        {owner}
        {repo}
        issueNumber={number}
        milestoneId={issue.milestone_id}
        onChanged={loadIssue}
      />

      {#if issue.body}
        <CommentCard
          author={issue.author}
          createdAt={issue.created_at}
          body={issue.body}
          reactions={issueReactions}
          onToggleReaction={toggleIssueReaction}
        />
      {/if}

      <AttachmentPanel {owner} {repo} target="issues" targetId={number} />

      {#each commentList as comment (comment.id)}
        <CommentCard
          author={comment.author}
          createdAt={comment.created_at}
          body={comment.body}
          reactions={commentReactions[comment.id] || []}
          onToggleReaction={(content) => toggleCommentReaction(comment.id, content)}
        >
          {#snippet children()}
            <AttachmentPanel {owner} {repo} target="issues/comments" targetId={comment.id} />
          {/snippet}
        </CommentCard>
      {/each}

      <IssueCommentForm
        {owner}
        {repo}
        issueNumber={number}
        issueState={issue.state}
        onError={(message) => (error = message)}
        onChanged={loadIssue}
      />
    </div>
  {/if}
</div>

<style>
  .issue-detail { max-width: 800px; }

  .error-banner {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 10px 12px;
    border-radius: var(--radius);
    margin-bottom: 16px;
  }
</style>
