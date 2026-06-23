<script lang="ts">
  import { page } from '$app/stores';
  import { repos } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { onMount } from 'svelte';
  
  const t = createT();
  
  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let path = $derived($page.params.path!);
  let branch = $derived($page.url.searchParams.get('ref') || 'main');
  let sha = $derived($page.url.searchParams.get('sha') || '');
  
  let fileContent = $state('');
  let commitMessage = $state('');
  let targetBranch = $state(branch);
  let loading = $state(true);
  let saving = $state(false);
  let error = $state('');
  let success = $state(false);
  
  // Determine if this is a new file or editing existing
  let isNew = $derived(!sha);
  
  onMount(async () => {
    if (!isNew) {
      // Load existing file content
      try {
        const data = await repos.blob(owner, repo, path, branch);
        if (data.content) {
          fileContent = data.content;
          commitMessage = `Update ${path}`;
        }
      } catch (err) {
        error = `Failed to load file: ${err instanceof Error ? err.message : String(err)}`;
      }
    } else {
      commitMessage = `Create ${path || 'new file'}`;
    }
    loading = false;
  });
  
  async function saveFile() {
    if (!fileContent.trim()) {
      error = 'File content cannot be empty';
      return;
    }
    
    if (!commitMessage.trim()) {
      error = 'Commit message is required';
      return;
    }
    
    saving = true;
    error = '';
    success = false;
    
    try {
      const payload = {
        branch: targetBranch,
        content: fileContent,
        message: commitMessage,
        ...(isNew ? {} : { sha }),
      };
      
      await repos.saveContent(owner, repo, path, payload);
      success = true;
      setTimeout(() => {
        window.location.href = `/${owner}/${repo}/blob/${path}`;
      }, 1500);
    } catch (err) {
      error = `Error: ${err instanceof Error ? err.message : String(err)}`;
    } finally {
      saving = false;
    }
  }
</script>

{#if loading}
  <div class="loading">Loading...</div>
{:else}
  <div class="editor-container">
    <div class="editor-header">
      <h1>{isNew ? t('repo.new_file') || 'New File' : t('repo.edit_file') || 'Edit File'}</h1>
      <div class="file-path">{path || '(new file)'}</div>
    </div>
    
    {#if error}
      <div class="error-message">{error}</div>
    {/if}
    
    {#if success}
      <div class="success-message">File saved successfully! Redirecting...</div>
    {/if}
    
    <div class="editor-form">
      <div class="form-group">
        <label for="file-content">Content</label>
        <textarea 
          id="file-content" 
          bind:value={fileContent} 
          class="file-editor"
          rows="20"
          placeholder="Enter file content..."
        ></textarea>
      </div>
      
      <div class="form-group">
        <label for="commit-message">Commit Message</label>
        <input 
          type="text" 
          id="commit-message" 
          bind:value={commitMessage}
          class="commit-input"
          placeholder="Enter commit message..."
        />
      </div>
      
      <div class="form-group">
        <label for="branch">Branch</label>
        <input 
          type="text" 
          id="branch" 
          bind:value={targetBranch}
          class="branch-input"
          placeholder="Branch name..."
        />
      </div>
      
      <div class="form-actions">
        <button 
          class="btn-save" 
          onclick={saveFile}
          disabled={saving}
        >
          {saving ? 'Saving...' : (isNew ? 'Create File' : 'Save Changes')}
        </button>
        <a href="/{owner}/{repo}" class="btn-cancel">Cancel</a>
      </div>
    </div>
  </div>
{/if}

<style>
  .loading {
    padding: 2rem;
    text-align: center;
    color: #666;
  }
  
  .editor-container {
    max-width: 900px;
    margin: 0 auto;
    padding: 2rem;
  }
  
  .editor-header {
    margin-bottom: 2rem;
  }
  
  .editor-header h1 {
    margin: 0 0 0.5rem 0;
    font-size: 1.5rem;
  }
  
  .file-path {
    color: #666;
    font-family: monospace;
    font-size: 0.9rem;
  }
  
  .error-message {
    background: #fee;
    color: #c33;
    padding: 1rem;
    border-radius: 4px;
    margin-bottom: 1rem;
  }
  
  .success-message {
    background: #efe;
    color: #3c3;
    padding: 1rem;
    border-radius: 4px;
    margin-bottom: 1rem;
  }
  
  .editor-form {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
  }
  
  .form-group {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  
  .form-group label {
    font-weight: 600;
    font-size: 0.9rem;
  }
  
  .file-editor {
    width: 100%;
    min-height: 400px;
    padding: 1rem;
    font-family: 'Monaco', 'Menlo', 'Ubuntu Mono', monospace;
    font-size: 0.9rem;
    line-height: 1.5;
    border: 1px solid #ddd;
    border-radius: 4px;
    resize: vertical;
  }
  
  .commit-input,
  .branch-input {
    width: 100%;
    padding: 0.75rem;
    font-size: 0.9rem;
    border: 1px solid #ddd;
    border-radius: 4px;
  }
  
  .form-actions {
    display: flex;
    gap: 1rem;
    margin-top: 1rem;
  }
  
  .btn-save {
    padding: 0.75rem 1.5rem;
    background: #2da44e;
    color: white;
    border: none;
    border-radius: 4px;
    font-weight: 600;
    cursor: pointer;
  }
  
  .btn-save:hover:not(:disabled) {
    background: #218838;
  }
  
  .btn-save:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  
  .btn-cancel {
    padding: 0.75rem 1.5rem;
    background: #f6f8fa;
    color: #24292f;
    border: 1px solid #d0d7de;
    border-radius: 4px;
    font-weight: 600;
    text-decoration: none;
    display: inline-flex;
    align-items: center;
  }
  
  .btn-cancel:hover {
    background: #eaeef2;
  }
</style>
