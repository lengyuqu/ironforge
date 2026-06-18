#!/usr/bin/env node

const BACKEND_URL = (process.env.BACKEND_URL || 'http://127.0.0.1:8080').replace(/\/$/, '');
const API_BASE = `${BACKEND_URL}/api/v1`;
const OPENAPI_URL = `${BACKEND_URL}/api-docs/openapi.json`;

const SAMPLE_PARAMS = {
  owner: 'testuser',
  repo: 'testrepo',
  name: 'demo',
  title: 'readme',
  number: '1',
  id: '1',
  sha: 'main',
  ref: 'main',
  path: 'README.md',
  format: 'npm',
  username: 'testuser',
  branch: 'main',
  base_branch: 'main',
  head_branch: 'main',
  new_name: 'demo',
  file: 'README.md',
  tag: 'v1.0.0',
  issue_number: '1',
  pull_number: '1',
};

const SKIP_METHODS = new Set(['head']);
const TIMEOUT_MS = Number(process.env.OPENAPI_SMOKE_TIMEOUT_MS || 10000);

const checks = [];
let failed = 0;

function sampleForParam(name) {
  const key = String(name).replace(/\./g, '_').toLowerCase();
  if (key === '...path') return 'docs/README.md';
  if (SAMPLE_PARAMS[key]) return SAMPLE_PARAMS[key];

  if (key.endsWith('_name')) {
    const base = key.replace(/_name$/, '');
    return SAMPLE_PARAMS[base] || 'demo';
  }

  if (/^(\d+|id|number|num|page|limit|offset)/.test(key)) return '1';
  if (key.includes('sha') || key.includes('ref')) return 'main';
  if (key.includes('branch')) return 'main';
  if (key.includes('path')) return 'README.md';
  if (key.includes('format')) return 'npm';
  if (key.includes('owner') || key.includes('user') || key.includes('org')) return 'testuser';
  return 'demo';
}

function resolvePath(pathTemplate) {
  return pathTemplate.replace(/\{([^}]+)\}/g, (_m, p1) => {
    const [name] = String(p1).split(':');
    return encodeURIComponent(sampleForParam(name));
  });
}

function shouldAuth(operation, openapiDoc) {
  const hasRequiredSecurity = (list) => {
    if (!Array.isArray(list)) return false;
    return list.some((entry) => entry && Object.keys(entry).length > 0);
  };

  if (operation.security !== undefined) {
    return hasRequiredSecurity(operation.security);
  }

  return hasRequiredSecurity(openapiDoc.security);
}

function isNonEmptyJson(obj) {
  if (!obj || typeof obj !== 'object') return false;
  const content = obj.content;
  return !!content && typeof content === 'object' && Object.keys(content).length > 0;
}

async function requestWithTimeout(url, init) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), TIMEOUT_MS);
  try {
    const res = await fetch(url, {
      ...init,
      signal: controller.signal,
    });
    return { ok: true, response: res };
  } catch (err) {
    return { ok: false, error: err };
  } finally {
    clearTimeout(timer);
  }
}

async function ensureToken() {
  const username = `smoke_${Date.now()}_${Math.floor(Math.random() * 10000)}`;
  const email = `${username}@example.com`;
  const password = 'Qz7$wRtm';
  const registerPayload = {
    username,
    email,
    password,
  };

  const registerUrl = `${API_BASE}/users/register`;
  const regResp = await requestWithTimeout(registerUrl, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(registerPayload),
  });

  if (!regResp.ok) {
    checks.push(`⚠️ 用户注册失败: ${regResp.error.message}`);
    return null;
  }

  if (regResp.response.status === 201 || regResp.response.status === 200) {
    const body = await regResp.response.json().catch(() => ({}));
    if (body?.token) return body.token;
  }

  const loginResp = await requestWithTimeout(`${API_BASE}/users/login`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ login: username, password }),
  });

  if (!loginResp.ok) {
    checks.push(`⚠️ 生成 Token 失败：${loginResp.error.message}`);
    return null;
  }

  if (loginResp.response.status < 400) {
    const loginBody = await loginResp.response.json().catch(() => ({}));
    return loginBody?.token || null;
  }

  checks.push(`⚠️ 生成 Token 失败：HTTP ${loginResp.response.status}`);
  return null;
}

function payloadForOperation(method, operation) {
  if (!['post', 'put', 'patch', 'delete'].includes(method)) return null;
  if (!operation?.requestBody || !isNonEmptyJson(operation.requestBody)) return null;
  const content = operation.requestBody.content || {};
  if (content['application/json']) return JSON.stringify({});
  return null;
}

function buildAuthHeaders(operation, openapiDoc, token) {
  if (!shouldAuth(operation, openapiDoc) || !token) return {};
  return { authorization: `Bearer ${token}` };
}

function isFailureStatus(status) {
  return status >= 500;
}

console.log('接口全量冒烟开始');
console.log(`backend: ${BACKEND_URL}`);
console.log(`openapi: ${OPENAPI_URL}`);

const response = await requestWithTimeout(OPENAPI_URL, { method: 'GET' });
if (!response.ok) {
  console.log(`❌ 读取 OpenAPI 规范失败: ${response.error?.message || `HTTP ${response.response?.status}`}`);
  process.exit(1);
}

if (!response.response.ok) {
  console.log(`❌ OpenAPI 规范返回异常: HTTP ${response.response.status}`);
  process.exit(1);
}

const openapi = await response.response.json().catch(() => ({}));
const paths = openapi.paths || {};

const token = await ensureToken();
if (token) {
  checks.push('✅ 已生成 JWT token，受保护接口将带鉴权头重放');
} else {
  checks.push('⚠️ 受保护接口将不带鉴权头执行（部分接口会返回 401）');
}

const entries = Object.entries(paths);
let total = 0;

for (const [rawPath, methods] of entries) {
  for (const [method, operation] of Object.entries(methods || {})) {
    const lower = String(method).toLowerCase();
    if (!['get', 'post', 'put', 'patch', 'delete', 'head', 'options'].includes(lower)) continue;
    if (SKIP_METHODS.has(lower)) continue;

    total += 1;
    const resolvedPath = resolvePath(rawPath);
    const url = `${BACKEND_URL}${resolvedPath}`;
    const headers = {
      ...buildAuthHeaders(operation, openapi, token),
    };
    if (lower !== 'get' && lower !== 'head' && lower !== 'options') {
      headers['content-type'] = 'application/json';
    }

    const body = payloadForOperation(lower, operation);
    const req = await requestWithTimeout(url, {
      method: lower.toUpperCase(),
      headers: Object.keys(headers).length > 0 ? headers : undefined,
      body,
    });

    if (!req.ok) {
      checks.push(`❌ ${lower.toUpperCase()} ${resolvedPath}: ${req.error.message}`);
      failed += 1;
      continue;
    }

    if (isFailureStatus(req.response.status)) {
      checks.push(`❌ ${lower.toUpperCase()} ${resolvedPath}: HTTP ${req.response.status}`);
      failed += 1;
    } else {
      checks.push(`✅ ${lower.toUpperCase()} ${resolvedPath}: HTTP ${req.response.status}`);
    }
  }
}

for (const line of checks) {
  console.log(line);
}

if (failed > 0) {
  console.log(`\n❌ OpenAPI 接口冒烟失败: ${failed}/${total} 个异常`);
  process.exit(1);
}

console.log(`\n✅ OpenAPI 接口冒烟通过: ${total} 个已请求`);
process.exit(0);
