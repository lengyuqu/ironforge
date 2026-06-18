#!/usr/bin/env node
// Headless console smoke test.
//
// Loads a list of routes in headless Chrome and reports any uncaught
// exceptions, console.error output, or failed network requests. Catches
// runtime-only bugs (e.g. Svelte runes leaking into a plain .ts module ->
// "$state is not defined") across EVERY page, not just whichever one you
// happened to open by hand.
//
// Usage:
//   node scripts/console-smoke.mjs                 # default base + route list
//   BASE=http://localhost:8080 node scripts/console-smoke.mjs /login /dashboard
//
// Exit code 0 = all clean, 1 = errors found (CI-friendly).
//
// Requires Google Chrome installed; no npm dependencies (uses Node's built-in
// fetch + WebSocket, Node >= 21).

import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const BASE = process.env.BASE || 'http://localhost:8080';
const PORT = Number(process.env.CDP_PORT || 9223);
const WAIT_MS = Number(process.env.WAIT_MS || 4000);
// Expected noise while crawling logged-out: the browser logs a generic
// "Failed to load resource" console.error for every 401/403 fetch.
const IGNORE = [
  /Failed to load resource.*\b(401|403)\b/,
  /the server responded with a status of (401|403)/,
];

const ROUTES = process.argv.slice(2).length
  ? process.argv.slice(2)
  : [
      '/', '/login', '/register', '/dashboard', '/notifications',
      '/search', '/orgs', '/forgot-password',
      '/admin', '/admin/users', '/admin/orgs', '/admin/audit', '/admin/settings',
    ];

const CHROME = process.env.CHROME ||
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

const profile = mkdtempSync(join(tmpdir(), 'cdp-smoke-'));
const chrome = spawn(CHROME, [
  '--headless=new', '--disable-gpu', '--no-sandbox',
  `--remote-debugging-port=${PORT}`, `--user-data-dir=${profile}`, 'about:blank',
], { stdio: 'ignore' });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const cleanup = () => { try { chrome.kill('SIGKILL'); } catch {} try { rmSync(profile, { recursive: true, force: true }); } catch {} };

async function waitDebugger() {
  for (let i = 0; i < 50; i++) {
    try { await fetch(`http://localhost:${PORT}/json/version`); return; } catch {}
    await sleep(200);
  }
  throw new Error('Chrome debugger did not come up');
}

async function checkRoute(path) {
  const target = await (await fetch(
    `http://localhost:${PORT}/json/new?${encodeURIComponent(BASE + path)}`,
    { method: 'PUT' },
  )).json();
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  const problems = [];
  let id = 0;
  const send = (method, params) => ws.send(JSON.stringify({ id: ++id, method, params }));
  await new Promise((res) => { ws.onopen = res; });
  send('Runtime.enable');
  send('Log.enable');
  send('Network.enable');
  ws.onmessage = (m) => {
    const d = JSON.parse(m.data);
    if (d.method === 'Runtime.exceptionThrown') {
      const e = d.params.exceptionDetails;
      problems.push('  ✖ EXCEPTION: ' + (e.exception?.description || e.text).split('\n')[0]);
    } else if (d.method === 'Log.entryAdded' && d.params.entry.level === 'error') {
      const txt = d.params.entry.text;
      if (!IGNORE.some((re) => re.test(txt))) problems.push('  ✖ console.error: ' + txt);
    } else if (d.method === 'Network.responseReceived') {
      const { url, status } = d.params.response;
      // 401/403 on API calls are expected auth-gating while crawling logged-out.
      const authGate = (status === 401 || status === 403) && url.includes('/api/');
      if (status >= 400 && !authGate && !IGNORE.some((re) => re.test(url)))
        problems.push(`  ✖ HTTP ${status}: ${url}`);
    }
  };
  await sleep(WAIT_MS);
  // Close the tab so it doesn't keep running.
  try { await fetch(`http://localhost:${PORT}/json/close/${target.id}`); } catch {}
  ws.close();
  return problems;
}

let failed = 0;
try {
  await waitDebugger();
  console.log(`Console smoke against ${BASE} (${ROUTES.length} routes)\n`);
  for (const r of ROUTES) {
    const problems = await checkRoute(r);
    if (problems.length) { failed++; console.log(`✗ ${r}`); console.log(problems.join('\n')); }
    else console.log(`✓ ${r}`);
  }
  console.log(`\n${failed ? `❌ ${failed} route(s) with errors` : '✅ all routes clean'}`);
} finally {
  cleanup();
}
process.exit(failed ? 1 : 0);
