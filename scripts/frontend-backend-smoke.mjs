#!/usr/bin/env node
// Frontend + backend smoke checks for automated integration.
//
// 1) Backend health endpoint: /health
// 2) Backend public API endpoint: /api/v1/repos/explore
// 3) Frontend home route accessibility
// 4) Frontend search route accessibility
// 5) Frontend search query route accessibility
//
// Optional env:
//   BACKEND_URL=http://127.0.0.1:8080
//   FRONTEND_URL=http://127.0.0.1:5173
//   API_BASE=http://127.0.0.1:8080/api/v1
//
// Exit code 1 if any check fails.

const BACKEND_URL = (process.env.BACKEND_URL || 'http://127.0.0.1:8080').replace(/\/$/, '');
const FRONTEND_URL = (process.env.FRONTEND_URL || 'http://127.0.0.1:5173').replace(/\/$/, '');
const API_BASE = (process.env.API_BASE || `${BACKEND_URL}/api/v1`).replace(/\/$/, '');

const checks = [];
let failed = 0;

async function requestJson(name, url, init) {
  try {
    const res = await fetch(url, init);
    const body = await res.json().catch(() => null);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    checks.push(`✅ ${name}: ${res.status}`);
    return { ok: true, status: res.status, body };
  } catch (e) {
    checks.push(`❌ ${name}: ${e.message}`);
    failed += 1;
    return { ok: false, body: null };
  }
}

function checkHtml(name, body) {
  if (typeof body !== 'string' || !body.toLowerCase().includes('<html')) {
    checks.push(`❌ ${name}: response is not HTML`);
    failed += 1;
    return;
  }
  checks.push(`✅ ${name}: html loaded`);
}

console.log('Integration smoke start');
console.log(`backend: ${BACKEND_URL}`);
console.log(`frontend: ${FRONTEND_URL}`);
console.log(`api: ${API_BASE}`);

const health = await requestJson('GET /health', `${BACKEND_URL}/health`);
if (health.ok && health.body && health.body.status !== 'healthy') {
  checks.push(`❌ /health: status=${health.body.status}`);
  failed += 1;
} else if (health.ok) {
  checks.push(`✅ /health: status=${health.body?.status || 'ok'}`);
}

await requestJson('GET /api/v1/repos/explore', `${API_BASE}/repos/explore?per_page=1`);

try {
  const homeRes = await fetch(`${FRONTEND_URL}/`);
  const homeText = await homeRes.text();
  if (!homeRes.ok) {
    checks.push(`❌ frontend home: HTTP ${homeRes.status}`);
    failed += 1;
  } else {
    checkHtml('frontend home', homeText);
  }
} catch (e) {
  checks.push(`❌ frontend home: ${e.message}`);
  failed += 1;
}

try {
  const searchRes = await fetch(`${FRONTEND_URL}/search`);
  const searchText = await searchRes.text();
  if (!searchRes.ok) {
    checks.push(`❌ frontend search: HTTP ${searchRes.status}`);
    failed += 1;
  } else {
    checkHtml('frontend search', searchText);
  }
} catch (e) {
  checks.push(`❌ frontend search: ${e.message}`);
  failed += 1;
}

try {
  const searchQueryRes = await fetch(`${FRONTEND_URL}/search?q=readme&type=all`);
  const searchQueryText = await searchQueryRes.text();
  if (!searchQueryRes.ok) {
    checks.push(`❌ frontend search query: HTTP ${searchQueryRes.status}`);
    failed += 1;
  } else {
    checkHtml('frontend search query', searchQueryText);
  }
} catch (e) {
  checks.push(`❌ frontend search query: ${e.message}`);
  failed += 1;
}

for (const line of checks) {
  console.log(line);
}

if (failed > 0) {
  console.log(`\n❌ ${failed} check(s) failed`);
  process.exit(1);
}

console.log('\n✅ integration checks passed');
process.exit(0);
