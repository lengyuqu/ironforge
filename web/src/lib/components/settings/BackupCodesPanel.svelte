<script lang="ts">
  import { toast } from '$lib/components/toast.svelte';

  let {
    codes,
  }: {
    codes: string[];
  } = $props();

  async function copyBackupCodes() {
    if (codes.length === 0) return;
    try {
      await navigator.clipboard.writeText(codes.join('\n'));
      toast.success('Backup codes copied');
    } catch {
      toast.error('Copy failed');
    }
  }
</script>

<section class="section backup-section" aria-label="New backup codes">
  <div class="section-heading">
    <div>
      <h2>Backup Codes</h2>
      <p>Each code can be used once if you lose authenticator access.</p>
    </div>
    <button type="button" class="btn btn-secondary" onclick={copyBackupCodes}>Copy Codes</button>
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
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-secondary);
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
    font-size: 18px;
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

  .btn {
    width: fit-content;
    padding: 8px 14px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    cursor: pointer;
    font-weight: 600;
  }

  .btn-secondary {
    background: var(--bg-primary);
    color: var(--text-primary);
  }

  @media (max-width: 720px) {
    .section-heading {
      display: grid;
      grid-template-columns: 1fr;
    }
  }
</style>
