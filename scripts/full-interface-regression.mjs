#!/usr/bin/env node

import { spawnSync } from 'node:child_process';

const ROOT = process.cwd();
const BACKEND_URL = (process.env.BACKEND_URL || 'http://127.0.0.1:8080').replace(/\/$/, '');
const FRONTEND_URL_CANDIDATES = process.env.FRONTEND_URL
  ? [process.env.FRONTEND_URL]
  : ['http://127.0.0.1:4173', 'http://127.0.0.1:5173'];

function run(label, command, args, options = {}) {
  const env = { ...process.env, ...(options.env || {}) };
  const result = spawnSync(command, args, {
    cwd: options.cwd || ROOT,
    env,
    stdio: 'inherit',
    shell: false,
  });

  if (result.error) {
    console.error(`❌ ${label} 启动失败: ${result.error.message}`);
    return false;
  }

  if (result.status !== 0) {
    console.error(`❌ ${label} 失败（exit ${result.status}）`);
    return false;
  }

  console.log(`✅ ${label}`);
  return true;
}

function canPing(url) {
  return fetch(url, { method: 'GET' })
    .then((res) => res.ok)
    .catch(() => false);
}

async function firstReachableUrl(urls) {
  for (const url of urls) {
    const normalized = url.replace(/\/$/, '');
    if (await canPing(`${normalized}/`)) return normalized;
  }
  return null;
}

async function main() {
  const allOk = [];

  console.log('=== 后端回归 ===');
  allOk.push(run('cargo test (rg-http 全量接口测试)', 'cargo', ['test', '-p', 'rg-http', '--', '--nocapture']));
  allOk.push(run('cargo test (workspace 核心回归)', 'cargo', ['test', '--workspace', '--', '--nocapture']));

  console.log('\n=== 前端静态回归 ===');
  allOk.push(run('web npm run check', 'npm', ['run', 'check'], { cwd: `${ROOT}/web` }));
  allOk.push(run('web npm run build', 'npm', ['run', 'build'], { cwd: `${ROOT}/web` }));

  const backendOk = await canPing(`${BACKEND_URL}/health`);
  const frontendUrl = await firstReachableUrl(FRONTEND_URL_CANDIDATES);
  const frontendOk = !!frontendUrl;

  if (backendOk) {
    console.log('\n=== 后端运行态接口回归（需后端可达）===');
    allOk.push(
      run(
        'openapi 路径级接口冒烟',
        'node',
        ['scripts/openapi-interface-smoke.mjs'],
        { env: { BACKEND_URL } },
      ),
    );
  }

  if (backendOk && frontendOk) {
    console.log('\n=== 前端运行态接口回归（后端+前端均可达）===');
    allOk.push(
      run(
        '前后端联调基础可达性',
        'node',
        ['scripts/frontend-backend-smoke.mjs'],
        { env: { BACKEND_URL, FRONTEND_URL: frontendUrl } },
      ),
    );
    allOk.push(
      run(
        '前端关键页面运行时冒烟（console/network）',
        'node',
        ['scripts/console-smoke.mjs'],
        { env: { BASE: frontendUrl } },
      ),
    );
  } else {
    console.log('\n⚠️ 跳过部分运行态冒烟：');
    if (!backendOk) console.log(`  - 后端未就绪：${BACKEND_URL}/health`);
    if (!frontendOk) console.log(`  - 前端未就绪：${FRONTEND_URL_CANDIDATES.map((url) => `${url.replace(/\/$/, '')}/`).join(', ')}`);
    console.log('  启动服务后可设置 BACKEND_URL/FRONTEND_URL 进行完整接口回归');
  }

  const failed = allOk.some((v) => !v);
  if (failed) {
    console.error('\n❌ 全链路接口回归未通过');
    process.exit(1);
  }

  console.log('\n✅ 全链路接口回归通过');
}

await main();
