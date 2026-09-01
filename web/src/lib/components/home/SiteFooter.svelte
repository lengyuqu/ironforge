<script lang="ts">
  // Landing site footer — logo, links and copyright line.
  import { createT } from '$lib/i18n';

  const t = createT();
</script>

<footer class="footer">
  <div class="footer-container">
    <div class="footer-logo">
      <span class="footer-logo-text">IronForge</span>
    </div>
    <div class="footer-links">
      <a href="https://github.com/lengyuqu/ironforge" target="_blank" rel="noreferrer">{t('home.footer.github')}</a>
      <a href="/explore">{t('home.footer.explore')}</a>
      <a href="https://github.com/lengyuqu/ironforge#readme" target="_blank" rel="noreferrer">{t('home.footer.help')}</a>
    </div>
    <div class="footer-copyright">
      © 2026 IronForge. {t('home.footer.built_with')}
    </div>
  </div>
</footer>

<style>
  .footer {
    background: var(--bg-secondary, #f6f8fa);
    border-top: 1px solid var(--border, #d0d7de);
    padding: 40px 24px;
  }

  .footer-container {
    max-width: 1200px;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
  }

  .footer-logo-text {
    font-size: 20px;
    font-weight: 700;
    color: var(--text-primary, #1f2328);
  }

  .footer-links {
    display: flex;
    gap: 24px;
  }

  .footer-links a {
    color: var(--text-secondary, #656d76);
    text-decoration: none;
    font-size: 14px;
  }

  .footer-links a:hover {
    color: var(--primary, #2da44e);
  }

  .footer-copyright {
    font-size: 12px;
    color: var(--text-muted, #8b949e);
  }
</style>
