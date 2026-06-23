<script lang="ts">
  import { page } from '$app/stores';
  import { repos } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { onMount } from 'svelte';
  
  const t = createT();
  
  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let path = $derived($page.url.searchParams.get('path') || '');
  
  let fileContent = $state('');
  let commitMessage = $state('');
  let targetBranch = $state('main');
  let filePath = $state(path || '');
  let loading = $state(false);
  let saving = $state(false);
  let error = $state('');
  let success = $state(false);
  
  onMount(() => {
    commitMessage = `Create ${filePath || 'new file'}`;
  });
  
  async function saveFile() {
    if (!filePath.trim()) {
      error = 'File path is required';
      return;
    }
    
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
      };
      
      await repos.saveContent(owner, repo, filePath, payload);
      success = true;
      setTimeout(() => {
        window.location.href = `/${owner}/${repo}/blob/${filePath}`;
      }, 1500);
    } catch (err) {
      error = `Error: ${err instanceof Error ? err.message : String(err)}`;
    } finally {
      saving = false;
    }
  }
</script>

<div class="editor-container">
  <div class="editor-header">
    <h1>{t('repo.new_file') || 'New File'}</h1>
    <div class="file-path">{filePath || '(new file)'}</div>
  </div>
  
  {#if error}
    <div class="error-message">{error}</div>
  {/if}
  
  {#if success}
    <div class="success-message">File created successfully! Redirecting...</div>
  {/if}
  
  <div class="editor-form">
    <div class="form-group">
      <label for="file-path-input">File Path</label>
      <input 
        type="text" 
        id="file-path-input" 
        bind:value={filePath}
        class="path-input"
        placeholder="e.g., README.md, docs/api.md"
      />
    </div>
    
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
        {saving ? 'Creating...' : 'Create File'}
      </button>
      <a href="/{owner}/{repo}" class="btn-cancel">Cancel</a>
    </div>
  </div>
</div>

<style>
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
  
  .path-input,
  .file-editor,
  .commit-input,
  .branch-input {
    width: 100%;
    padding: 0.75rem;
    font-size: 0.9rem;
    border: 1px solid #ddd;
    border-radius: 4px;
  }
  
  .file-editor {
    min-height: 400px;
    font-family: 'Monaco', 'Menlo', 'Ubuntu Mono', monospace;
    line-height: 1.5;
    resize: vertical;
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
