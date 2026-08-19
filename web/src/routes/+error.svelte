<script lang="ts">
  import { page } from '$app/stores';
  import { createT } from '$lib/i18n';

  const t = createT();

  const status = $derived($page.status);
  const error = $derived($page.error);

  const titleKey = $derived(
    status === 404
      ? 'error_page.not_found_title'
      : status >= 500
        ? 'error_page.server_error_title'
        : 'error_page.generic_title'
  );

  const descKey = $derived(
    status === 404
      ? 'error_page.not_found_desc'
      : status >= 500
        ? 'error_page.server_error_desc'
        : 'error_page.generic_desc'
  );
</script>

<div class="error-page">
  <div class="error-code">{status}</div>
  <h1>{t(titleKey)}</h1>
  <p>{t(descKey)}</p>
  {#if error?.message}
    <p class="error-detail">{error.message}</p>
  {/if}
  <a href="/" class="back-home">{t('error_page.back_home')}</a>
</div>

<style>
  .error-page {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-height: 60vh;
    text-align: center;
    padding: 2rem;
  }

  .error-code {
    font-size: 6rem;
    font-weight: 700;
    color: var(--color-primary, #0969da);
    line-height: 1;
    margin-bottom: 1rem;
  }

  h1 {
    font-size: 1.75rem;
    margin: 0 0 0.75rem;
  }

  p {
    color: var(--color-text-secondary, #57606a);
    margin: 0 0 0.5rem;
    max-width: 480px;
  }

  .error-detail {
    color: var(--color-danger, #cf222e);
    font-family: monospace;
    font-size: 0.875rem;
    word-break: break-word;
  }

  .back-home {
    display: inline-block;
    margin-top: 1.5rem;
    padding: 0.5rem 1.25rem;
    background-color: var(--color-primary, #0969da);
    color: #fff;
    text-decoration: none;
    border-radius: 6px;
    font-weight: 500;
    transition: opacity 0.15s;
  }

  .back-home:hover {
    opacity: 0.9;
  }
</style>
