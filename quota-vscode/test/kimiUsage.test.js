const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');

const {
  kimiUsageErrorMessage,
  kimiCodeHome,
  localCredentialsPath,
  normalizeUsageResponse,
  readLocalCredentials,
} = require('../out/kimiUsage');

const NOW = Math.floor(Date.now() / 1000);

function withTempCredentials(content, run) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'quota-kimi-'));
  const credentialsPath = path.join(dir, 'kimi-code.json');
  try {
    if (content != null) fs.writeFileSync(credentialsPath, content);
    return run(credentialsPath);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

test('normalizeUsageResponse parses all four windows with reset times', () => {
  const summary = normalizeUsageResponse({
    usages: {
      limit_5h: { used_ratio: 0.18, reset_time: '2026-10-02T16:18:57.800Z' },
      limit_7d: { used_ratio: 0.5, reset_time: '2026-10-05T00:00:00.000Z' },
      limit_month_total: { used_ratio: '0.755', reset_time: '2026-11-01T00:00:00Z' },
      limit_month_code: { used_ratio: 1.2 },
    },
  });

  assert.deepEqual(summary.windows, [
    { label: '5h', remainingPercent: 82, resetAt: Date.parse('2026-10-02T16:18:57.800Z') },
    { label: 'Wk', remainingPercent: 50, resetAt: Date.parse('2026-10-05T00:00:00.000Z') },
    { label: 'Month', remainingPercent: 25, resetAt: Date.parse('2026-11-01T00:00:00Z') },
    { label: 'Code', remainingPercent: 0, resetAt: undefined },
  ]);
  assert.equal(summary.note, undefined);
});

test('normalizeUsageResponse tolerates missing windows and reports no data', () => {
  assert.deepEqual(normalizeUsageResponse({ usages: {} }), { windows: [], note: 'No usage data' });
  assert.deepEqual(normalizeUsageResponse({}), { windows: [], note: 'No usage data' });
  assert.deepEqual(
    normalizeUsageResponse({ usages: { limit_5h: { used_ratio: 'not a number' } } }),
    { windows: [], note: 'No usage data' },
  );

  const partial = normalizeUsageResponse({ usages: { limit_5h: { used_ratio: 0.994 } } });
  assert.equal(partial.windows.length, 1);
  assert.equal(partial.windows[0].remainingPercent, 1);
  assert.equal(partial.note, undefined);
});

test('normalizeUsageResponse rejects partial and empty string ratios', () => {
  // Rust's trim().parse::<f64>() rejects the whole string; so must we.
  for (const usedRatio of ['0.2junk', '', '   ']) {
    const summary = normalizeUsageResponse({ usages: { limit_5h: { used_ratio: usedRatio } } });
    assert.deepEqual(summary, { windows: [], note: 'No usage data' }, `used_ratio: ${JSON.stringify(usedRatio)}`);
  }
});

test('normalizeUsageResponse rejects non-decimal prefixes Rust f64 would reject', () => {
  // Number() alone accepts these; Rust's f64 parser does not.
  for (const usedRatio of ['0x10', '0b10', '0o10']) {
    const summary = normalizeUsageResponse({ usages: { limit_5h: { used_ratio: usedRatio } } });
    assert.deepEqual(summary, { windows: [], note: 'No usage data' }, `used_ratio: ${JSON.stringify(usedRatio)}`);
  }
});

test('readLocalCredentials returns the token for a valid session', () => {
  withTempCredentials(JSON.stringify({ access_token: 'test-secret', expires_at: NOW + 3600 }), (credentialsPath) => {
    const credentials = readLocalCredentials(credentialsPath, NOW);
    assert.equal(credentials.accessToken, 'test-secret');
    assert.equal(credentials.expiresAt, NOW + 3600);
  });
});

test('readLocalCredentials points expired, missing, and malformed files at /login', () => {
  for (const content of [
    null,
    'not json at all',
    JSON.stringify({ access_token: '', expires_at: NOW + 3600 }),
    JSON.stringify({ access_token: 'test-secret' }),
    JSON.stringify({ access_token: 'test-secret', expires_at: -5 }),
    JSON.stringify({ access_token: 'test-secret', expires_at: NOW - 1 }),
  ]) {
    withTempCredentials(content, (credentialsPath) => {
      assert.throws(
        () => readLocalCredentials(credentialsPath, NOW),
        (error) => {
          assert.match(error.message, /login|Kimi Code/);
          assert.equal(error.message.includes('test-secret'), false);
          return true;
        },
      );
    });
  }
});

test('credential errors never quote the file content', () => {
  withTempCredentials('{"access_token": "super-secret", "expires_at": ', (credentialsPath) => {
    assert.throws(
      () => readLocalCredentials(credentialsPath, NOW),
      (error) => {
        assert.equal(error.message.includes('super-secret'), false);
        return true;
      },
    );
  });
});

test('kimiCodeHome honours KIMI_CODE_HOME and defaults to ~/.kimi-code', () => {
  const original = process.env.KIMI_CODE_HOME;
  try {
    process.env.KIMI_CODE_HOME = '/tmp/kimi-home';
    assert.equal(kimiCodeHome(), '/tmp/kimi-home');
    assert.equal(localCredentialsPath(), path.join('/tmp/kimi-home', 'credentials', 'kimi-code.json'));
    delete process.env.KIMI_CODE_HOME;
    assert.equal(kimiCodeHome(), path.join(os.homedir(), '.kimi-code'));
  } finally {
    if (original == null) delete process.env.KIMI_CODE_HOME;
    else process.env.KIMI_CODE_HOME = original;
  }
});

test('kimiUsageErrorMessage redacts rejected tokens and other failures', () => {
  for (const status of [401, 403]) {
    const message = kimiUsageErrorMessage(status);
    assert.match(message, /\/login/);
    assert.match(message, /Kimi Code/);
  }
  assert.equal(kimiUsageErrorMessage(500), 'Kimi Code usage request failed with HTTP 500.');
});
