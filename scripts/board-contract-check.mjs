#!/usr/bin/env node

import { readFileSync } from 'node:fs';

const files = {
  client: 'web/src/lib/api/client.svelte.ts',
  boardsPage: 'web/src/routes/[owner]/[repo]/boards/+page.svelte',
  issueBoardPage: 'web/src/routes/[owner]/[repo]/issues/board/+page.svelte',
  backend: 'crates/rg-http/src/api/boards.rs',
  backendService: 'crates/rg-core/src/board/service.rs',
};

const source = Object.fromEntries(
  Object.entries(files).map(([key, file]) => [key, readFileSync(file, 'utf8')]),
);

const checks = [
  {
    name: 'backend create-card request accepts note, not title',
    ok:
      /pub struct CreateCardRequest[\s\S]*pub note: Option<String>/.test(source.backend) &&
      !/pub struct CreateCardRequest[\s\S]*pub title:/.test(source.backend),
  },
  {
    name: 'API client createCard payload does not expose title',
    ok:
      /createCard:[\s\S]*data: \{ note\?: string; issue_id\?: number \}/.test(source.client) &&
      !/createCard:[\s\S]*data: \{ title:/.test(source.client),
  },
  {
    name: 'API client moveCard requires position',
    ok: /moveCard:[\s\S]*data: \{ column_id: number; position: number \}/.test(source.client),
  },
  {
    name: 'standalone board page fetches full board before rendering columns',
    ok:
      /async function selectBoard\(board: any\)[\s\S]*boards\.get\(owner, repo, board\.id\)/.test(source.boardsPage) &&
      /function normalizeColumns\(board: any\)/.test(source.boardsPage),
  },
  {
    name: 'standalone board page renders card note instead of absent title',
    ok: /card\.note \|\| card\.issue\?\.title/.test(source.boardsPage) && !/<span>\{card\.title\}<\/span>/.test(source.boardsPage),
  },
  {
    name: 'backend board response enriches cards with issue metadata',
    ok:
      /pub struct CardFull[\s\S]*#\[serde\(flatten\)\][\s\S]*pub card: Card[\s\S]*pub issue: Option<Issue>/.test(source.backendService) &&
      /pub cards: Vec<CardFull>/.test(source.backendService),
  },
  {
    name: 'issue board links cards by issue number, not database issue_id',
    ok:
      /href=\{`\/\$\{owner\}\/\$\{repo\}\/issues\/\$\{card\.issue\.number\}`\}/.test(source.issueBoardPage) &&
      !/href=\{`\/\$\{owner\}\/\$\{repo\}\/issues\/\$\{card\.issue_id\}`\}/.test(source.issueBoardPage),
  },
  {
    name: 'board card creation pages send note payloads',
    ok:
      /createCard\(owner, repo, activeBoard\.id, colId, \{\s*note: newCardTitle\.trim\(\),\s*\}\)/.test(source.boardsPage) &&
      /createCard\(owner, repo, activeBoardId!, colId, \{ note \}\)/.test(source.issueBoardPage),
  },
];

let failed = 0;
for (const check of checks) {
  if (check.ok) {
    console.log(`✅ ${check.name}`);
  } else {
    console.log(`❌ ${check.name}`);
    failed += 1;
  }
}

if (failed > 0) {
  console.error(`\nBoard contract check failed: ${failed} issue(s)`);
  process.exit(1);
}

console.log('\nBoard contract check passed');
