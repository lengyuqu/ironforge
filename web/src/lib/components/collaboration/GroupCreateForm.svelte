<script lang="ts">
  // Create a coordination group anchored at this repository (needs write access
  // to it). Title and description are readable by EVERYONE who can read this
  // repository's collaboration data, including people without access to the
  // repositories the group later links to; the form says so up front.
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import Button from '$lib/components/Button.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import { collabGroups, GROUP_LIMITS, type GroupView } from '$lib/api/collabGroups';
  import { newClientId } from '$lib/utils/collaboration';
  import { groupProblem } from '$lib/utils/collabGroups';

  interface Props {
    owner: string;
    repo: string;
    onCreated: (group: GroupView) => void;
  }

  let { owner, repo, onCreated }: Props = $props();
  const t = createT();

  let title = $state('');
  let description = $state('');
  let key = $state(newClientId());
  let busy = $state(false);
  let error = $state('');
  let touched = $state(false);

  let problem = $derived(groupProblem(title, description));

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    touched = true;
    if (busy || problem) return;
    busy = true;
    error = '';
    try {
      const group = await collabGroups.create(owner, repo, {
        client_request_id: key,
        title: title.trim(),
        description,
      });
      title = '';
      description = '';
      touched = false;
      key = newClientId();
      onCreated(group);
    } catch (err) {
      error = toErrorMessage(err, t('common.unknown_error'));
      // A new attempt after a failure is a new request, not a replay.
      key = newClientId();
    } finally {
      busy = false;
    }
  }
</script>

<form class="form" onsubmit={submit} data-testid="group-create-form">
  <p class="warn" data-testid="sharing-warning">{t('collab_groups.sharing_warning')}</p>
  <label class="field">
    <span class="form-label">{t('collab_groups.create.title_label')}</span>
    <input class="form-input" bind:value={title} maxlength={GROUP_LIMITS.titleChars} autocomplete="off" />
  </label>
  <label class="field">
    <span class="form-label">{t('collab_groups.create.description_label')}</span>
    <textarea class="form-input" rows="3" bind:value={description}></textarea>
  </label>
  {#if touched && problem}<p class="problem" role="alert">{t(`collab_groups.create.${problem}`)}</p>{/if}
  {#if error}<ErrorBanner message={error} />{/if}
  <div>
    <Button type="submit" variant="primary" disabled={busy}>
      {busy ? t('common.loading') : t('collab_groups.create.submit')}
    </Button>
  </div>
  <p class="muted">{t('collab_groups.create.write_needed')}</p>
</form>

<style>
  .form { display: flex; flex-direction: column; gap: 10px; max-width: 70ch; }
  .field { display: flex; flex-direction: column; gap: 4px; }
  .form-label { font-size: 13px; font-weight: 600; }
  .warn { margin: 0; padding: 8px 10px; font-size: 12px; background: var(--yellow-subtle); border: 1px solid var(--yellow-dim); border-radius: 6px; color: var(--text-secondary); }
  .problem { margin: 0; font-size: 12px; color: var(--red); }
  .muted { margin: 0; font-size: 12px; color: var(--text-muted); }
</style>
