<script lang="ts">
  import { isLoggedIn, getUser } from '$lib/stores/auth';
</script>

<div class="hero">
  <div class="hero-content">
    <svg viewBox="0 0 16 16" width="64" height="64" fill="var(--accent)">
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/>
    </svg>
    <h1>IronForge</h1>
    <p class="tagline">Self-hosted Git platform built with Rust</p>

    <div class="features">
      <div class="feature">
        <span class="feature-icon">🔒</span>
        <strong>Secure</strong>
        <span>Argon2 + JWT authentication</span>
      </div>
      <div class="feature">
        <span class="feature-icon">⚡</span>
        <strong>Fast</strong>
        <span>Rust-powered HTTP & SSH</span>
      </div>
      <div class="feature">
        <span class="feature-icon">🔀</span>
        <strong>Code Review</strong>
        <span>PR reviews & branch protection</span>
      </div>
      <div class="feature">
        <span class="feature-icon">▶️</span>
        <strong>CI/CD</strong>
        <span>Built-in pipeline engine</span>
      </div>
    </div>

    <div class="cta">
      {#if isLoggedIn()}
        <a href="/dashboard" class="btn-primary">Go to Dashboard →</a>
      {:else}
        <a href="/login" class="btn-primary">Sign in</a>
        <a href="/register" class="btn-secondary">Create account</a>
      {/if}
    </div>
  </div>
</div>

<style>
  .hero {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: calc(100vh - 62px);
    padding: 40px 24px;
  }

  .hero-content {
    text-align: center;
    max-width: 600px;
  }

  h1 {
    font-size: 42px;
    font-weight: 800;
    margin: 16px 0 8px;
    background: linear-gradient(135deg, var(--accent), var(--purple));
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
  }

  .tagline {
    font-size: 18px;
    color: var(--text-secondary);
    margin-bottom: 40px;
  }

  .features {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
    margin-bottom: 40px;
  }

  .feature {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 16px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
  }
  .feature-icon { font-size: 24px; }
  .feature strong { font-size: 14px; }
  .feature span:last-child { font-size: 12px; color: var(--text-secondary); }

  .cta {
    display: flex;
    justify-content: center;
    gap: 12px;
  }

  .btn-primary {
    display: inline-flex;
    align-items: center;
    padding: 10px 24px;
    background: var(--green-dim);
    color: #fff;
    border-radius: var(--radius);
    font-weight: 600;
    font-size: 16px;
    text-decoration: none;
  }
  .btn-primary:hover { background: var(--green); text-decoration: none; }

  .btn-secondary {
    display: inline-flex;
    align-items: center;
    padding: 10px 24px;
    background: none;
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-weight: 600;
    font-size: 16px;
    text-decoration: none;
  }
  .btn-secondary:hover { background: var(--bg-hover); text-decoration: none; }
</style>
