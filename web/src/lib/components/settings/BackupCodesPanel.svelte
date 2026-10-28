<script lang="ts">
  import { toast } from '$lib/components/toast.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    codes,
  }: {
    codes: string[];
  } = $props();

  async function copyBackupCodes() {
    if (codes.length === 0) return;
    try {
      await navigator.clipboard.writeText(codes.join('\n'));
      toast.success(t('settings.mfa.codes_copied'));
    } catch {
      toast.error(t('settings.mfa.copy_failed'));
    }
  }
</script>

<section class="section backup-section surface" aria-label={t('settings.mfa.new_codes')}>
  <div class="section-heading surface">
    <div>
      <h2>{t('settings.mfa.codes_title')}</h2>
      <p>{t('settings.mfa.codes_desc')}</p>
    </div>
    <button type="button" class="btn btn-secondary" onclick={copyBackupCodes}>{t('settings.mfa.copy_codes')}</button>
  </div>
  <div class="code-grid">
    {#each codes as code}
      <code>{code}</code>
    {/each}
  </div>
</section>

<style>
  .section {
    margin-bottom: 20px;
    padding: 20px;
  }

  .section-heading {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 18px;
  }

  h2 {
    margin: 0;
  }

  p {
    margin: 0;
    color: var(--text-secondary);
  }

  .code-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 8px;
  }

  .code-grid code {
    display: block;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-primary);
  }

  @media (max-width: 720px) {
    .section-heading {
      display: grid;
      grid-template-columns: 1fr;
    }
  }
</style>
