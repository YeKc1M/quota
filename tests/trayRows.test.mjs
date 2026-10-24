import assert from 'node:assert/strict';
import test from 'node:test';
import { buildTrayUsageRows } from '../src/data/trayRows.ts';

const order = ['githubCopilot', 'codex', 'antigravity', 'claude', 'kiro', 'cursor', 'grok', 'opencodeGo', 'kimi'];

function emptyAccounts() {
  return {
    githubCopilot: [],
    codex: [],
    antigravity: [],
    claude: [],
    kiro: [],
    cursor: [],
    grok: [],
    opencodeGo: [],
    kimi: [],
  };
}

test('builds compact rows in the saved provider order', () => {
  const accounts = emptyAccounts();
  accounts.codex.push({
    id: 'codex-1',
    email: 'dev@example.com',
    authMode: 'oauth',
    quota: { hourlyRemainingPercent: 82.4, weeklyRemainingPercent: 63.6 },
    requiresReauthentication: false,
    createdAt: 1,
    lastUsed: 1,
  });
  accounts.cursor.push({
    id: 'cursor-1',
    email: 'cursor@example.com',
    source: 'oauth',
    totalPercent: 41.2,
    autoPercent: 30,
    apiPercent: 11,
    onDemandEnabled: false,
    createdAt: 1,
    lastUsed: 1,
  });

  assert.deepEqual(buildTrayUsageRows(accounts, ['cursor', 'codex'], 'remaining'), [
    'Cursor · cursor@example.com · Total 59% left · Auto 70% left · API 89% left · On-demand off',
    'Codex · dev@example.com · 5h 82% left · Week 64% left',
  ]);
  assert.deepEqual(buildTrayUsageRows(accounts, ['cursor', 'codex'], 'used'), [
    'Cursor · cursor@example.com · Total 41% used · Auto 30% used · API 11% used · On-demand off',
    'Codex · dev@example.com · 5h 18% used · Week 36% used',
  ]);
});

test('clamps percentages and keeps count-based Copilot usage', () => {
  const accounts = emptyAccounts();
  accounts.githubCopilot.push({
    id: 'copilot-1',
    githubLogin: 'octocat',
    usage: {
      inlineSuggestionsUsedPercent: -5,
      chatMessagesUsedPercent: 104,
      premiumRequestsUsedPercent: null,
      inlineIncluded: false,
      chatIncluded: false,
      premiumIncluded: false,
      usedPremiumRequests: 12,
      totalPremiumRequests: 300,
    },
    createdAt: 1,
    lastUsed: 1,
  });

  assert.deepEqual(buildTrayUsageRows(accounts, order, 'used'), [
    'Copilot · @octocat · Premium 12/300 used · Chat 100% used · Inline 0% used',
  ]);
});

test('keeps connected accounts visible when usage has not loaded', () => {
  const accounts = emptyAccounts();
  accounts.claude.push({
    id: 'claude-1',
    email: 'claude@example.com',
    authMode: 'oauth',
    quota: {},
    requiresReauthentication: true,
    createdAt: 1,
    lastUsed: 1,
  });

  assert.deepEqual(buildTrayUsageRows(accounts, order, 'remaining'), [
    'Claude · claude@example.com · No usage data yet',
  ]);
});

test('formats Kimi Code windows in used and remaining modes', () => {
  const accounts = emptyAccounts();
  accounts.kimi.push({
    id: 'kimi_local',
    label: 'Kimi Code',
    usage: {
      windows: [
        { label: '5h', remainingPercent: 82, resetAt: null },
        { label: 'Wk', remainingPercent: 64, resetAt: null },
        { label: 'Month', remainingPercent: 25, resetAt: null },
        { label: 'Code', remainingPercent: 9, resetAt: null },
      ],
      note: null,
    },
    quotaQueryLastError: null,
    quotaQueryLastErrorAt: null,
    usageUpdatedAt: null,
    createdAt: 1,
    lastUsed: 1,
  });

  assert.deepEqual(buildTrayUsageRows(accounts, order, 'remaining'), [
    'Kimi · Kimi Code · 5h 82% left · Wk 64% left · Month 25% left · Code 9% left',
  ]);
  assert.deepEqual(buildTrayUsageRows(accounts, order, 'used'), [
    'Kimi · Kimi Code · 5h 18% used · Wk 36% used · Month 75% used · Code 91% used',
  ]);
});

test('keeps Kimi Code connected when usage windows are empty', () => {
  const accounts = emptyAccounts();
  accounts.kimi.push({
    id: 'kimi_local',
    label: 'Kimi Code',
    usage: { windows: [], note: 'No usage data' },
    quotaQueryLastError: null,
    quotaQueryLastErrorAt: null,
    usageUpdatedAt: null,
    createdAt: 1,
    lastUsed: 1,
  });

  assert.deepEqual(buildTrayUsageRows(accounts, order, 'remaining'), [
    'Kimi · Kimi Code · No usage data yet',
  ]);
});

test('orders Kimi rows by the saved provider order', () => {
  const accounts = emptyAccounts();
  accounts.kimi.push({
    id: 'kimi_local',
    label: 'Kimi Code',
    usage: {
      windows: [{ label: '5h', remainingPercent: 55, resetAt: null }],
      note: null,
    },
    quotaQueryLastError: null,
    quotaQueryLastErrorAt: null,
    usageUpdatedAt: null,
    createdAt: 1,
    lastUsed: 1,
  });
  accounts.codex.push({
    id: 'codex-1',
    email: 'dev@example.com',
    authMode: 'oauth',
    quota: { hourlyRemainingPercent: 90, weeklyRemainingPercent: 80 },
    requiresReauthentication: false,
    createdAt: 1,
    lastUsed: 1,
  });

  assert.deepEqual(buildTrayUsageRows(accounts, ['kimi', 'codex'], 'remaining'), [
    'Kimi · Kimi Code · 5h 55% left',
    'Codex · dev@example.com · 5h 90% left · Week 80% left',
  ]);
});
