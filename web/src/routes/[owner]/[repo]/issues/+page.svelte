<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import Pagination from '$lib/components/Pagination.svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { issues, type IssueConfig, type IssueTemplate } from '$lib/api/client.svelte';
  import type { Issue } from '$lib/types/entities';
  import IssueFilterTabs from '$lib/components/issues/IssueFilterTabs.svelte';
  import IssueList from '$lib/components/issues/IssueList.svelte';
  import IssueTemplateChooser from '$lib/components/issues/IssueTemplateChooser.svelte';
  import IssueCreateForm from '$lib/components/issues/IssueCreateForm.svelte';
  import { milestones } from '$lib/api/client.svelte';
  import type { Milestone } from '$lib/types/entities';
  import { createT } from '$lib/i18n';
  import { createLatest } from '$lib/utils/latest';
  import { buildQueryHref, readEnumParam, readPageParam } from '$lib/utils/urlQuery';

  const t = createT();
  const PER_PAGE = 20;
  const ISSUE_STATES = ['open', 'closed', 'all'] as const;

  let owner = $derived(page.params.owner!);
  let repo = $derived(page.params.repo!);

  let issueList = $state<Issue[]>([]);
  let loading = $state(true);
  let error = $state('');
  let filterState = $derived(readEnumParam(page.url, 'state', 'open', ISSUE_STATES));
  // Milestone filter (M2-2 A1): '' = no constraint, 'none' = no milestone, else String(id).
  let milestoneFilter = $derived(page.url.searchParams.get('milestone') ?? '');
  let currentPage = $derived(readPageParam(page.url));
  let total = $state(0);
  let milestoneList = $state<Milestone[]>([]);

  const issuesLatest = createLatest();
  const milestonesLatest = createLatest();

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
    void loadIssues();
  });

  $effect(() => {
    void loadMilestones();
  });

  async function loadIssues() {
    const reqOwner = owner;
    const reqRepo = repo;
    const reqState = filterState;
    const reqMilestone = milestoneFilter;
    const reqPage = currentPage;
    const isCurrent = issuesLatest.begin();
    try {
      loading = true;
      error = '';
      const res = await issues.list(
        reqOwner,
        reqRepo,
        reqState,
        reqPage,
        PER_PAGE,
        undefined,
        undefined,
        reqMilestone || undefined,
      );
      if (!isCurrent()) return;
      issueList = res.data;
      total = res.pagination?.total ?? res.data.length;
      if (reqPage > 1 && res.data.length === 0 && total > 0) {
        void goto(buildQueryHref(page.url, { page: null }), {
          replaceState: true,
          keepFocus: true,
          noScroll: true,
        });
      }
    } catch (e) {
      if (!isCurrent()) return;
      error = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      if (isCurrent()) {
        loading = false;
      }
    }
  }

  async function loadMilestones() {
    const reqOwner = owner;
    const reqRepo = repo;
    const isCurrent = milestonesLatest.begin();
    try {
      const list = await milestones.list(reqOwner, reqRepo);
      if (!isCurrent()) return;
      milestoneList = list;
    } catch {
      if (!isCurrent()) return;
      // Milestone filter is progressive enhancement — hide the row on failure.
      milestoneList = [];
    }
  }

  function changeFilter(next: string) {
    void goto(buildQueryHref(page.url, { state: next === 'open' ? null : next, page: null }), {
      keepFocus: true,
      noScroll: true,
    });
  }

  function changeMilestone(next: string) {
    void goto(buildQueryHref(page.url, { milestone: next || null, page: null }), {
      keepFocus: true,
      noScroll: true,
    });
  }

  function changePage(nextPage: number) {
    void goto(buildQueryHref(page.url, { page: nextPage === 1 ? null : nextPage }), {
      keepFocus: true,
    });
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

<div class="gh-toolbar issues-toolbar">
  <IssueFilterTabs
    filter={filterState}
    onFilterChange={changeFilter}
    milestones={milestoneList}
    milestoneFilter={milestoneFilter}
    onMilestoneChange={changeMilestone}
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
  <ErrorBanner message={error} />
{/if}

<IssueList {owner} {repo} issues={issueList} {loading} filter={filterState} />

<Pagination {total} page={currentPage} perPage={PER_PAGE} onPageChange={changePage} />

<style>
  .issues-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

</style>
