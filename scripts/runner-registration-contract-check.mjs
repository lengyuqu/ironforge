#!/usr/bin/env node

import { readFileSync } from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const clientPath = path.join(root, 'web/src/lib/api/client.svelte.ts');
const pagePath = path.join(root, 'web/src/routes/[owner]/[repo]/settings/runners/+page.svelte');
const backendPath = path.join(root, 'crates/rg-http/src/api/runners.rs');

const client = readFileSync(clientPath, 'utf8');
const page = readFileSync(pagePath, 'utf8');
const backend = readFileSync(backendPath, 'utf8');

const failures = [];

if (!/pub\s+struct\s+RegisterRunnerResponse\s*\{[\s\S]*\btoken:\s+String[\s\S]*\}/.test(backend)) {
  failures.push('Backend runner registration response must include the runner token');
}

if (!/export\s+interface\s+RegisterRunnerResponse\s*\{[\s\S]*token:\s*string[\s\S]*\}/.test(client)) {
  failures.push('API client must type the runner registration token response');
}

if (!/register:\s*\([^)]*\)\s*=>\s*\n?\s*request<RegisterRunnerResponse>\('\/runners\/register'/.test(client)) {
  failures.push('API client runners.register must return RegisterRunnerResponse');
}

if (!/const\s+res\s*=\s*await\s+runners\.register\(/.test(page)) {
  failures.push('Runner settings page must capture the registration response');
}

if (!/registeredRunner\s*=\s*\{[\s\S]*token:\s*res\.token[\s\S]*\}/.test(page)) {
  failures.push('Runner settings page must store the returned runner token');
}

if (!/\{registeredRunner\.token\}/.test(page)) {
  failures.push('Runner settings page must render the returned runner token');
}

if (!/navigator\.clipboard\.writeText\(registeredRunner\.token\)/.test(page)) {
  failures.push('Runner settings page must provide a copy action for the token');
}

if (failures.length > 0) {
  for (const failure of failures) {
    console.log(`FAIL ${failure}`);
  }
  process.exit(1);
}

console.log('Runner registration frontend/backend contract ok');
