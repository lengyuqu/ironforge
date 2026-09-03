<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { issues, type IssueConfig, type IssueTemplate } from '$lib/api/client.svelte';
  import type { Issue } from '$lib/types/entities';
import IssueFilterTabs from '$lib/components/issues/IssueFilterTabs.svelte';
import IssueList from '$lib/components/issues/IssueList.svelte';
import IssueTemplateChooser from '$lib/components/issues/IssueTemplateChooser.svelte';
import IssueCreateForm from '$lib/components/issues/IssueCreateForm.svelte';
import { milestones } from '$lib/api/client.svelte';
import type { Milestone } from '$lib/types/entities';
import { createT } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  let issueList = $state<Issue[]>([]);
  let loading = $state(true);
  let error = $state('');
  let filterState = $state('open');
  // Milestone filter (M2-2 A1): '' = no constraint, 'none' = no milestone, else String(id).
  let milestoneFilter = $state('');
  let milestoneList = $state<Milestone[]>([]);

  let showCreate = $state(false);
  let showChooser = $state(false);
  let templatesLoaded = $state(false);
  let issueTemplates = $state<IssueTemplate[]>([]);
  let templateConfig = $state<IssueConfig>({ blank_issues_enabled: true, contact_links: [] });
  let configWarning = $state('');

  // 模板预填字段（chooseTemplate 时写入，传给 CreateForm 作为初始值）
  let draftTitle = $state('');
  let draftBody = $state('');
  let draftLabels = $state('');
  let draftAssignees = $state('');
  let draftKey = $state(0);

  $effect(() => {
    loadIssues();
    loadMilestones();
  });

  async function loadIssues() {
    try {
      loading = true;
      error = '';
      issueList = (
        await issues.list(owner, repo, filterState, undefined, undefined, undefined, undefined, milestoneFilter || undefined)
      ).data;
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function loadMilestones() {
    try {
      milestoneList = await milestones.list(owner, repo);
    } catch {
      // Milestone filter is progressive enhancement — hide the row on failure.
      milestoneList = [];
    }
  }

  async function openCreate() {
    if (showCreate || showChooser) {
      showCreate = false;
      showChooser = false;
      return;
    }
    try {
      if (!templatesLoaded) {
        [issueTemplates, templateConfig] = await Promise.all([
          issues.templates(owner, repo),
          issues.templateConfig(owner, repo),
        ]);
        templatesLoaded = true;
      }
      if (issueTemplates.length > 0 || templateConfig.contact_links.length > 0) {
        showChooser = true;
      } else {
        startCreate();
      }
    } catch {
      // Templates/config unreadable — degrade to defaults and surface the
      // validation error so the admin can fix .github/ISSUE_TEMPLATE/config.yml.
      issueTemplates = [];
      templateConfig = { blank_issues_enabled: true, contact_links: [] };
      configWarning = await loadConfigError();
      showChooser = true;
    }
  }

  async function loadConfigError(): Promise<string> {
    try {
      const result = await issues.validateTemplateConfig(owner, repo);
      if (!result.valid && result.message) return result.message;
    } catch {
      // validate endpoint itself unavailable — generic message
    }
    return t('issues.templates.load_failed', 'Failed to load issue templates.');
  }

  function chooseTemplate(template?: IssueTemplate) {
    draftTitle = template?.title || '';
    draftBody = template?.content || '';
    draftLabels = template?.labels?.join(', ') || '';
    draftAssignees = template?.assignees?.join(', ') || '';
    draftKey += 1;
    showChooser = false;
    showCreate = true;
  }

  function startCreate() {
    draftTitle = '';
    draftBody = '';
    draftLabels = '';
    draftAssignees = '';
    draftKey += 1;
    showCreate = true;
  }

  function handleFormCancel() {
    showCreate = false;
    // 有模板/链接时退回选择器，否则直接关闭
    if (issueTemplates.length > 0 || templateConfig.contact_links.length > 0) {
      showChooser = true;
    }
  }

  async function handleCreated() {
    showCreate = false;
    await loadIssues();
  }
</script>

<svelte:head>
  <title>{t('issues.title')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="issues" starsCount={0} />

  <div class="gh-toolbar issues-toolbar">
    <IssueFilterTabs
      filter={filterState}
      onFilterChange={(next) => {
        filterState = next;
        loadIssues();
      }}
      milestones={milestoneList}
      milestoneFilter={milestoneFilter}
      onMilestoneChange={(next) => {
        milestoneFilter = next;
        loadIssues();
      }}
    />
    <button class="btn-primary" onclick={openCreate}>
      {t('issues.new')}
    </button>
  </div>

  {#if showChooser}
    <IssueTemplateChooser
      templates={issueTemplates}
      config={templateConfig}
      warning={configWarning}
      onChoose={chooseTemplate}
      onClose={() => (showChooser = false)}
    />
  {/if}

  {#if showCreate}
    {#key draftKey}
      <IssueCreateForm
        {owner}
        {repo}
        initialTitle={draftTitle}
        initialBody={draftBody}
        initialLabels={draftLabels}
        initialAssignees={draftAssignees}
        onCreated={handleCreated}
        onCancel={handleFormCancel}
      />
    {/key}
  {/if}

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <IssueList {owner} {repo} issues={issueList} {loading} filter={filterState} />
</div>

<style>
  .issues-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  .btn-primary {
    padding: 6px 16px;
    background: var(--accent);
    color: #fff;
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }

  .btn-primary:hover {
    background: var(--accent-hover);
  }

  .error-banner {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 10px 12px;
    border-radius: var(--radius);
    margin-bottom: 16px;
  }
</style>
