<script lang="ts">
  import { repos } from '$lib/api/client.svelte';
  import { isLoggedIn } from '$lib/stores/auth.svelte';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();
  const loggedIn = isLoggedIn();

  const featureCards = [
    {
      icon: '🛡️',
      title: t('home.features.secure'),
      description: t('home.features.secure_desc'),
      href: '/dashboard',
    },
    {
      icon: '⚡',
      title: t('home.features.fast'),
      description: t('home.features.fast_desc'),
      href: '/explore',
    },
    {
      icon: '👀',
      title: t('home.features.code_review'),
      description: t('home.features.code_review_desc'),
      href: '/notifications',
    },
    {
      icon: '🚀',
      title: t('home.features.cicd'),
      description: t('home.features.cicd_desc'),
      href: '/admin',
    },
  ];

  const quickLinks = loggedIn
    ? [
      { href: '/dashboard', label: t('home.cta.go_dashboard'), desc: '管理仓库与项目' },
      { href: '/explore', label: t('home.explore.title'), desc: '查找公开仓库与模板' },
      { href: '/notifications', label: t('nav.notifications'), desc: '查看待处理事件' },
    ]
    : [
      { href: '/register', label: t('home.cta.create_account'), desc: '创建账户开始团队协作' },
      { href: '/login', label: t('common.sign_in'), desc: '登录后开始管理你的仓库' },
      { href: '/explore', label: t('home.explore.title'), desc: '先浏览公开仓库与示例项目' },
    ];

  let repoList = $state<any[]>([]);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    repos.explore(1, 24).then(r => {
      repoList = r.data;
    }).catch((e: any) => {
      error = e.message;
    }).finally(() => {
      loading = false;
    });
  });
</script>

<svelte:head>
  <title>IronForge · {t('explore.title')}</title>
</svelte:head>

<div class="page-container">
  <section class="hero">
    <div class="hero-content">
      <p class="hero-kicker">Git platform for engineering teams</p>
      <h1>IronForge</h1>
      <p class="hero-tagline">{t('home.tagline')}</p>
      <div class="hero-actions">
        {#each quickLinks as quick}
          <a class="btn btn-primary hero-btn" href={quick.href}>
            <span>{quick.label}</span>
            <small>{quick.desc}</small>
          </a>
        {/each}
      </div>
    </div>
    <div class="hero-stats">
      <div class="hero-stat">
        <span>Git + HTTP + SSH</span>
        <small>完整协议栈，支持团队协作与 CI</small>
      </div>
      <div class="hero-stat">
        <span>Issue / PR / Review</span>
        <small>从代码评审到分支保护一体化</small>
      </div>
      <div class="hero-stat">
        <span>Self-hosted Default</span>
        <small>默认自托管，内置镜像和包注册表</small>
      </div>
    </div>
  </section>

  <section class="feature-grid">
    {#each featureCards as card}
      <a class="feature-card" href={card.href}>
        <div class="f-icon">{card.icon}</div>
        <div class="f-body">
          <h2>{card.title}</h2>
          <p>{card.description}</p>
        </div>
        <span class="f-cta">→</span>
      </a>
    {/each}
  </section>

  <div class="section-head">
    <h1>{t('explore.title')}</h1>
    <p class="subtitle">
      {#if !loading}
        {t('explore.subtitle', { count: repoList.length })}
      {/if}
    </p>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">{t('common.loading')}</p>
  {:else if repoList.length === 0}
    <div class="empty">
      <p>{t('explore.empty')}</p>
    </div>
  {:else}
    <div class="repo-grid">
      {#each repoList as repo}
        <a href="/{repo.owner_name}/{repo.name}" class="repo-card">
          <div class="rc-icon">📂</div>
          <div class="rc-body">
            <div class="rc-name">{repo.owner_name}/{repo.name}</div>
            <div class="rc-desc">{repo.description || t('common.no_description')}</div>
            <div class="rc-meta">
              {repo.stars_count || 0} ⭐ · {t('common.updated', { date: formatDate(repo.updated_at) })}
            </div>
          </div>
        </a>
      {/each}
    </div>

    <div class="explore-footer">
      <a href="/explore" class="view-all-btn">{t('home.explore.view_all')} →</a>
    </div>
  {/if}
</div>

<style>
  .hero {
    border: 1px solid var(--border);
    background: linear-gradient(130deg, var(--bg-secondary), #0f1e2e);
    border-radius: var(--radius-lg);
    padding: 28px;
    margin-bottom: 28px;
    display: grid;
    grid-template-columns: 1.2fr 1fr;
    gap: 24px;
    align-items: center;
  }

  .hero-content h1 {
    font-size: 32px;
    margin-bottom: 8px;
    letter-spacing: 0.2px;
  }

  .hero-kicker {
    color: var(--accent);
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.12em;
    margin-bottom: 8px;
    font-weight: 600;
  }

  .hero-tagline {
    font-size: 15px;
    color: var(--text-secondary);
    margin: 4px 0 18px;
    max-width: 42ch;
  }

  .hero-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  .hero-btn {
    text-decoration: none;
    text-align: left;
    display: inline-flex;
    flex-direction: column;
    gap: 2px;
    align-items: flex-start;
    padding: 10px 14px;
    min-width: 180px;
  }

  .hero-btn small {
    color: var(--text-secondary);
    font-size: 11px;
    font-weight: normal;
    margin: 0;
  }

  .hero-stats {
    display: grid;
    gap: 10px;
  }

  .hero-stat {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 12px;
    background: rgba(13, 17, 23, 0.35);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .hero-stat span {
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 600;
  }

  .hero-stat small {
    color: var(--text-secondary);
    font-size: 12px;
  }

  .feature-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 12px;
    margin-bottom: 28px;
  }

  .feature-card {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-secondary);
    color: var(--text-primary);
    text-decoration: none;
    transition: border-color 0.15s, transform 0.15s, background 0.15s;
  }

  .feature-card:hover {
    border-color: var(--accent);
    background: var(--bg-hover);
    transform: translateY(-1px);
    text-decoration: none;
  }

  .f-icon {
    font-size: 18px;
    line-height: 1.2;
  }

  .f-body h2 {
    font-size: 14px;
    margin: 0 0 4px;
    color: var(--text-primary);
  }

  .f-body p {
    font-size: 12px;
    color: var(--text-secondary);
    margin: 0;
  }

  .f-cta {
    margin-left: auto;
    color: var(--accent);
    font-size: 16px;
    opacity: 0.85;
  }

  .section-head {
    margin-bottom: 24px;
  }

  h1 {
    font-size: 24px;
    margin-bottom: 4px;
  }

  .subtitle {
    font-size: 14px;
    color: var(--text-secondary);
    min-height: 20px;
  }

  .empty {
    text-align: center;
    padding: 60px 24px;
    color: var(--text-secondary);
  }

  .repo-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
  }

  @media (max-width: 900px) {
    .hero {
      grid-template-columns: 1fr;
    }

    .feature-grid {
      grid-template-columns: repeat(2, 1fr);
    }

    .repo-grid {
      grid-template-columns: repeat(2, 1fr);
    }
  }

  @media (max-width: 600px) {
    .hero {
      padding: 20px;
      gap: 16px;
    }

    .feature-grid {
      grid-template-columns: 1fr;
    }

    .repo-grid {
      grid-template-columns: 1fr;
    }
  }

  .repo-card {
    display: flex;
    gap: 12px;
    padding: 16px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    text-decoration: none;
    color: var(--text-primary);
    transition: border-color 0.15s;
  }

  .repo-card:hover {
    border-color: var(--accent);
    text-decoration: none;
  }

  .repo-card:hover .rc-name {
    color: var(--accent-hover);
  }

  .rc-icon {
    font-size: 20px;
    flex-shrink: 0;
  }

  .rc-body {
    flex: 1;
    min-width: 0;
  }

  .rc-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--accent);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rc-desc {
    font-size: 12px;
    color: var(--text-secondary);
    margin-top: 4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rc-meta {
    font-size: 11px;
    color: var(--text-muted);
    margin-top: 8px;
  }

  .explore-footer {
    text-align: center;
    margin-top: 32px;
  }

  .view-all-btn {
    display: inline-block;
    padding: 8px 24px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--accent);
    font-size: 14px;
    font-weight: 500;
    text-decoration: none;
  }

  .view-all-btn:hover {
    background: var(--bg-hover);
    text-decoration: none;
  }
</style>
