<script lang="ts">
  // Link a task and/or a commit of ANOTHER repository into the group.
  //
  // The target is typed as `owner/name` and resolved to its stable repository id
  // through the ordinary repository API; the group API refers to repositories by
  // id only. Adding needs write access to this (anchor) repository AND to the
  // target. A target that is missing and one the person cannot see answer
  // identically, so a failure message never says which it was.
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { toast } from '$lib/components/toast.svelte';
  import Button from '$lib/components/Button.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import { ApiError } from '$lib/api/_base.svelte';
  import { repos } from '$lib/api/repos';
  import { collabGroups } from '$lib/api/collabGroups';
  import { newClientId } from '$lib/utils/collaboration';
  import { linkProblem, parseRepoRef } from '$lib/utils/collabGroups';

  interface Props {
    owner: string;
    repo: string;
    groupId: string;
    /** The group version as last read; the server refuses the change if it moved. */
    version: number;
    /** Called after a successful add, or when the group must be re-read (version conflict). */
    onChanged: () => void;
  }

  let { owner, repo, groupId, version, onChanged }: Props = $props();
  const t = createT();

  let target = $state('');
  let taskId = $state('');
  let sha = $state('');
  let key = $state(newClientId());
  let busy = $state(false);
  let error = $state('');
  let touched = $state(false);

  let problem = $derived(linkProblem(target, taskId, sha));

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    touched = true;
    if (busy || problem) return;
    const ref = parseRepoRef(target)!;
    busy = true;
    error = '';
    try {
      let targetId: number;
      try {
        targetId = (await repos.get(ref.owner, ref.name)).id;
      } catch (err) {
        if (err instanceof ApiError && (err.status === 404 || err.status === 403)) {
          throw new Error(t('collab_groups.add.repo_not_found'));
        }
        throw err;
      }
      await collabGroups.addLink(owner, repo, groupId, {
        client_request_id: key,
        target_repo_id: targetId,
        target_task_id: taskId.trim() || null,
        commit_sha: sha.trim() || null,
        expected_group_version: version,
      });
      toast.success(t('collab_groups.add.added_toast'));
      target = '';
      taskId = '';
      sha = '';
      touched = false;
      key = newClientId();
      onChanged();
    } catch (err) {
      error = toErrorMessage(err, t('common.unknown_error'));
      // A new attempt after a failure is a new request, not a replay.
      key = newClientId();
      if (err instanceof ApiError && err.code === 'VERSION_CONFLICT') {
        error = t('collab_groups.conflict');
        onChanged();
      }
    } finally {
      busy = false;
    }
  }
</script>

<form class="form" onsubmit={submit} data-testid="group-add-link-form">
  <label class="field">
    <span class="form-label">{t('collab_groups.add.target_label')}</span>
    <input class="form-input mono" bind:value={target} placeholder="owner/name" autocomplete="off" />
    <span class="hint">{t('collab_groups.add.target_hint')}</span>
  </label>
  <label class="field">
    <span class="form-label">{t('collab_groups.add.task_label')}</span>
    <input class="form-input mono" bind:value={taskId} autocomplete="off" />
  </label>
  <label class="field">
    <span class="form-label">{t('collab_groups.add.commit_label')}</span>
    <input class="form-input mono" bind:value={sha} autocomplete="off" />
  </label>
  <p class="hint">{t('collab_groups.add.need_one_hint')}</p>
  {#if touched && problem}<p class="problem" role="alert">{t(`collab_groups.add.${problem}`)}</p>{/if}
  {#if error}<ErrorBanner message={error} />{/if}
  <div>
    <Button type="submit" variant="primary" disabled={busy}>
      {busy ? t('common.loading') : t('collab_groups.add.submit')}
    </Button>
  </div>
  <p class="hint">{t('collab_groups.add.write_both')}</p>
</form>

<style>
  .form { display: flex; flex-direction: column; gap: 10px; max-width: 70ch; }
  .field { display: flex; flex-direction: column; gap: 4px; }
  .form-label { font-size: 13px; font-weight: 600; }
  .mono { font-family: var(--font-mono); font-size: 12px; }
  .hint { margin: 0; font-size: 12px; color: var(--text-muted); }
  .problem { margin: 0; font-size: 12px; color: var(--red); }
</style>
