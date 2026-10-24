// @env node
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';

export interface KimiUsageWindow {
  label: string;
  remainingPercent?: number;
  /** Unix milliseconds. */
  resetAt?: number;
}

export interface KimiUsageSummary {
  windows: KimiUsageWindow[];
  note?: string;
}

export interface LocalKimiCredentials {
  accessToken: string;
  expiresAt: number;
}

export class LocalKimiCredentialError extends Error {}

const WINDOW_KEYS: Array<[string, string]> = [
  ['limit_5h', '5h'],
  ['limit_7d', 'Wk'],
  ['limit_month_total', 'Month'],
  ['limit_month_code', 'Code'],
];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * Strict full-string numeric parse, mirroring Rust's `trim().parse::<f64>()`:
 * any trailing junk (or an empty string) rejects the whole value instead of
 * reading a partial number out of it. The grammar is plain decimal float
 * only — Number() alone would also take hex/binary/octal prefixes like
 * "0x10", which Rust's f64 parser rejects.
 */
const DECIMAL_FLOAT_PATTERN = /^[+-]?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?$/;

function parseRatioString(value: string): number {
  const trimmed = value.trim();
  if (trimmed === '') return Number.NaN;
  if (!DECIMAL_FLOAT_PATTERN.test(trimmed)) return Number.NaN;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) ? parsed : Number.NaN;
}

function clampPercent(value: number): number {
  return Math.min(100, Math.max(0, Math.round(value)));
}

/** Mirrors quota-core's local.rs: $KIMI_CODE_HOME or ~/.kimi-code. */
export function kimiCodeHome(): string {
  const fromEnv = typeof process.env.KIMI_CODE_HOME === 'string'
    ? process.env.KIMI_CODE_HOME.trim()
    : '';
  return fromEnv || path.join(os.homedir(), '.kimi-code');
}

export function localCredentialsPath(): string {
  return path.join(kimiCodeHome(), 'credentials', 'kimi-code.json');
}

/**
 * Validate the locally stored Kimi Code OAuth credentials. Mirrors the
 * quota-core contract: expired, missing, and malformed files all produce the
 * redacted "run Kimi Code / /login" guidance, and parse errors never quote
 * the file content.
 */
export function readLocalCredentials(credentialsPath = localCredentialsPath(), nowSeconds = Math.floor(Date.now() / 1000)): LocalKimiCredentials {
  let raw: string;
  try {
    raw = fs.readFileSync(credentialsPath, 'utf8');
  } catch {
    throw new LocalKimiCredentialError('No Kimi Code OAuth credentials found. Run /login in Kimi Code first.');
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    const match = /position (\d+)/.exec(message);
    throw new LocalKimiCredentialError(
      match
        ? `Kimi Code credentials are not valid JSON (position ${match[1]}). Run /login in Kimi Code.`
        : 'Kimi Code credentials are not valid JSON. Run /login in Kimi Code.',
    );
  }
  if (!isRecord(parsed)) {
    throw new LocalKimiCredentialError('Kimi Code credentials are not in the expected shape. Run /login in Kimi Code.');
  }

  const accessToken = typeof parsed.access_token === 'string' ? parsed.access_token.trim() : '';
  if (!accessToken) {
    throw new LocalKimiCredentialError('Kimi Code credentials are not in the expected shape: no access token. Run /login in Kimi Code.');
  }
  const expiresAt = typeof parsed.expires_at === 'number' ? parsed.expires_at : Number.NaN;
  if (!Number.isFinite(expiresAt) || expiresAt <= 0) {
    throw new LocalKimiCredentialError('Kimi Code credentials are not in the expected shape: invalid expiry timestamp. Run /login in Kimi Code.');
  }
  if (expiresAt <= nowSeconds) {
    throw new LocalKimiCredentialError("Kimi Code's stored token has expired. Run Kimi Code to refresh it, or use /login.");
  }

  return { accessToken, expiresAt };
}

/**
 * Normalize `GET https://api.kimi.com/coding/v1/usages`. Mirrors
 * quota-core's local_usage.rs: `used_ratio` accepts numbers or numeric
 * strings, `reset_time` is RFC3339, remaining percent is rounded, and a body
 * without any window yields a "No usage data" note instead of fake zeros.
 */
export function normalizeUsageResponse(raw: unknown): KimiUsageSummary {
  const usages = isRecord(raw) && isRecord(raw.usages) ? raw.usages : {};

  const windows: KimiUsageWindow[] = [];
  for (const [key, label] of WINDOW_KEYS) {
    const entry = usages[key];
    if (!isRecord(entry)) continue;

    const ratio = typeof entry.used_ratio === 'number'
      ? entry.used_ratio
      : typeof entry.used_ratio === 'string'
        ? parseRatioString(entry.used_ratio)
        : Number.NaN;
    if (!Number.isFinite(ratio)) continue;

    const resetTime = typeof entry.reset_time === 'string' ? Date.parse(entry.reset_time) : Number.NaN;
    windows.push({
      label,
      remainingPercent: clampPercent((1 - ratio) * 100),
      resetAt: Number.isFinite(resetTime) ? resetTime : undefined,
    });
  }

  return {
    windows,
    note: windows.length === 0 ? 'No usage data' : undefined,
  };
}

/**
 * A user-readable message for a failed usage request. Never includes the
 * response body; 401/403 point back to the local Kimi Code login.
 */
export function kimiUsageErrorMessage(status: number): string {
  if (status === 401 || status === 403) {
    return 'Kimi Code rejected the stored token. Run Kimi Code to refresh it, or use /login.';
  }
  return `Kimi Code usage request failed with HTTP ${status}.`;
}
