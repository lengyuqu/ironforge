<script lang="ts">
  import { getUser, isLoggedIn, isAdmin, logout } from '$lib/stores/auth';
  import { locale, type Locale } from '$lib/i18n';

  let showUserMenu = $state(false);
  let showLangMenu = $state(false);

  function handleLogout() {
    logout();
    showUserMenu = false;
    window.location.href = '/login';
  }

  function clickOutside(e: MouseEvent) {
    if (showUserMenu && !(e.target as HTMLElement).closest('.user-menu-container')) {
      showUserMenu = false;
    }
    if (showLangMenu && !(e.target as HTMLElement).closest('.lang-menu-container')) {
      showLangMenu = false;
    }
  }

  function setLocale(newLocale: Locale) {
    locale.set(newLocale);
    showLangMenu = false;
  }
</script>

<svelte:window onclick={clickOutside} />

<nav class="navbar">
  <div class="navbar-left">
    <a href="/" class="logo">
      <svg viewBox="0 0 16 16" width="28" height="28" fill="currentColor">
        <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/>
      </svg>
      <span class="logo-text">IronForge</span>
    </a>
  </div>

  <div class="navbar-right">
    {#if isLoggedIn()}
      <a href="/notifications" class="nav-icon" title="Notifications">🔔</a>
      <a href="/orgs" class="nav-icon" title="Organizations">🏢</a>

      <!-- Language Switcher -->
      <div class="lang-menu-container" style="position:relative">
        <button class="lang-btn" onclick={() => showLangMenu = !showLangMenu}>
          {$locale === 'zh-CN' ? '中文' : 'EN'}
        </button>
        {#if showLangMenu}
          <div class="dropdown">
            <button onclick={() => setLocale('en')} class:active={$locale === 'en'}>English</button>
            <button onclick={() => setLocale('zh-CN')} class:active={$locale === 'zh-CN'}>中文</button>
          </div>
        {/if}
      </div>

      <div class="user-menu-container" style="position:relative">
        <button class="user-btn" onclick={() => showUserMenu = !showUserMenu}>
          <div class="avatar">
            {(getUser()?.username || '?')[0].toUpperCase()}
          </div>
          <span>{getUser()?.username}</span>
          <svg viewBox="0 0 16 16" width="12" height="12" fill="currentColor">
            <path d="m4.427 7.427 3.396 3.396a.25.25 0 0 0 .354 0l3.396-3.396A.25.25 0 0 0 11.396 7H4.604a.25.25 0 0 0-.177.427z"/>
          </svg>
        </button>
        {#if showUserMenu}
          <div class="dropdown">
            <a href="/dashboard">Dashboard</a>
            <a href="/notifications">Notifications</a>
            <a href="/orgs">Organizations</a>
            {#if isAdmin()}
              <a href="/admin" class="admin-link">⚙ Admin Panel</a>
            {/if}
            <button onclick={handleLogout}>Sign out</button>
          </div>
        {/if}
      </div>
    {:else}
      <a href="/login" class="btn-outline">Sign in</a>
    {/if}
  </div>
</nav>

<style>
  .navbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 24px;
    height: 62px;
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border);
    position: sticky;
    top: 0;
    z-index: 100;
  }

  .logo {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text-primary);
    text-decoration: none;
  }
  .logo:hover { text-decoration: none; }
  .logo-text {
    font-size: 18px;
    font-weight: 700;
    letter-spacing: -0.5px;
  }

  .navbar-right {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .user-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 4px 10px;
    color: var(--text-primary);
    font-size: 14px;
  }
  .user-btn:hover { background: var(--bg-hover); }

  .avatar {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--accent);
    color: #fff;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    font-weight: 700;
  }

  .dropdown {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: 4px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    min-width: 160px;
    box-shadow: 0 8px 24px rgba(0,0,0,0.4);
    z-index: 200;
    overflow: hidden;
  }
  .dropdown a, .dropdown button {
    display: block;
    width: 100%;
    padding: 8px 16px;
    color: var(--text-primary);
    background: none;
    border: none;
    text-align: left;
    font-size: 14px;
    cursor: pointer;
    text-decoration: none;
  }
  .dropdown a:hover, .dropdown button:hover {
    background: var(--bg-hover);
    text-decoration: none;
  }

  .btn-outline {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 5px 16px;
    color: var(--text-primary);
    background: none;
    font-size: 14px;
  }
  .btn-outline:hover { background: var(--bg-hover); text-decoration: none; }

  .nav-icon {
    font-size: 18px;
    text-decoration: none;
    line-height: 1;
    opacity: 0.8;
  }
  .nav-icon:hover { opacity: 1; text-decoration: none; }

  .lang-btn {
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 4px 10px;
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
  }
  .lang-btn:hover { background: var(--bg-hover); }

  .dropdown button.active {
    font-weight: 600;
    color: var(--accent);
  }
</style>
