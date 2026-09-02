<script lang="ts">
  import type { IssueConfig, IssueTemplate } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    templates,
    config,
    warning,
    onChoose,
    onClose,
  }: {
    templates: IssueTemplate[];
    config: IssueConfig;
    /** config 损坏时 validate 返回的具体错误（降级提示，不阻断 blank issue） */
    warning: string;
    onChoose: (template?: IssueTemplate) => void;
    onClose: () => void;
  } = $props();

  const hasTemplates = $derived(templates.length > 0 || config.contact_links.length > 0);
</script>

<div class="template-chooser gh-card">
  <div class="chooser-heading">
    <div>
      <h2>{t('issues.templates.title')}</h2>
      <p>{t('issues.templates.description')}</p>
    </div>
    <button class="btn-secondary" onclick={onClose}>{t('issues.create_form.cancel')}</button>
  </div>

  {#if warning}
    <div class="config-warning">
      {t('issues.templates.invalid_config', 'Issue template config is invalid:')} {warning}
    </div>
  {/if}

  <div class="template-list">
    {#each templates as template (template.file_name)}
      <div class="template-option">
        <div>
          <strong>{template.name}</strong>
          <p>{template.about}</p>
        </div>
        <button class="btn-primary" onclick={() => onChoose(template)}>
          {t('issues.templates.get_started')}
        </button>
      </div>
    {/each}

    {#if config.blank_issues_enabled}
      <div class="template-option">
        <div>
          <strong>{t('issues.templates.blank')}</strong>
          <p>{t('issues.templates.blank_about')}</p>
        </div>
        <button class="btn-secondary" onclick={() => onChoose()}>
          {t('issues.templates.open_blank')}
        </button>
      </div>
    {/if}

    {#each config.contact_links as link (link.url)}
      <div class="template-option">
        <div>
          <strong>{link.name}</strong>
          <p>{link.about}</p>
        </div>
        <a
          class="btn-secondary external-link"
          href={link.url}
          target="_blank"
          rel="noopener noreferrer"
        >
          {t('issues.templates.open_link')}
        </a>
      </div>
    {/each}
  </div>
</div>

<style>
  .template-chooser {
    padding: 20px;
    margin-bottom: 24px;
  }

  .chooser-heading {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    align-items: flex-start;
    margin-bottom: 14px;
  }

  .chooser-heading h2 {
    margin: 0 0 4px;
    font-size: 18px;
  }

  .chooser-heading p,
  .template-option p {
    margin: 0;
    color: var(--text-secondary);
    font-size: 13px;
  }

  .template-list {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .template-option {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 14px;
    border-bottom: 1px solid var(--border-light);
  }

  .template-option:last-child {
    border-bottom: 0;
  }

  .external-link {
    text-decoration: none;
    white-space: nowrap;
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
    white-space: nowrap;
  }

  .btn-secondary {
    padding: 6px 16px;
    background: none;
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 14px;
    cursor: pointer;
    white-space: nowrap;
  }

  .config-warning {
    margin-bottom: 12px;
    padding: 10px 12px;
    border: 1px solid #d29922;
    background: rgba(210, 153, 34, 0.1);
    color: #d29922;
    border-radius: 6px;
    font-size: 13px;
    word-break: break-word;
  }
</style>
