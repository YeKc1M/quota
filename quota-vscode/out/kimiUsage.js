"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.LocalKimiCredentialError = void 0;
exports.kimiCodeHome = kimiCodeHome;
exports.localCredentialsPath = localCredentialsPath;
exports.readLocalCredentials = readLocalCredentials;
exports.normalizeUsageResponse = normalizeUsageResponse;
exports.kimiUsageErrorMessage = kimiUsageErrorMessage;
// @env node
const fs = __importStar(require("node:fs"));
const os = __importStar(require("node:os"));
const path = __importStar(require("node:path"));
class LocalKimiCredentialError extends Error {
}
exports.LocalKimiCredentialError = LocalKimiCredentialError;
const WINDOW_KEYS = [
    ['limit_5h', '5h'],
    ['limit_7d', 'Wk'],
    ['limit_month_total', 'Month'],
    ['limit_month_code', 'Code'],
];
function isRecord(value) {
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
function parseRatioString(value) {
    const trimmed = value.trim();
    if (trimmed === '')
        return Number.NaN;
    if (!DECIMAL_FLOAT_PATTERN.test(trimmed))
        return Number.NaN;
    const parsed = Number(trimmed);
    return Number.isFinite(parsed) ? parsed : Number.NaN;
}
function clampPercent(value) {
    return Math.min(100, Math.max(0, Math.round(value)));
}
/** Mirrors quota-core's local.rs: $KIMI_CODE_HOME or ~/.kimi-code. */
function kimiCodeHome() {
    const fromEnv = typeof process.env.KIMI_CODE_HOME === 'string'
        ? process.env.KIMI_CODE_HOME.trim()
        : '';
    return fromEnv || path.join(os.homedir(), '.kimi-code');
}
function localCredentialsPath() {
    return path.join(kimiCodeHome(), 'credentials', 'kimi-code.json');
}
/**
 * Validate the locally stored Kimi Code OAuth credentials. Mirrors the
 * quota-core contract: expired, missing, and malformed files all produce the
 * redacted "run Kimi Code / /login" guidance, and parse errors never quote
 * the file content.
 */
function readLocalCredentials(credentialsPath = localCredentialsPath(), nowSeconds = Math.floor(Date.now() / 1000)) {
    let raw;
    try {
        raw = fs.readFileSync(credentialsPath, 'utf8');
    }
    catch {
        throw new LocalKimiCredentialError('No Kimi Code OAuth credentials found. Run /login in Kimi Code first.');
    }
    let parsed;
    try {
        parsed = JSON.parse(raw);
    }
    catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        const match = /position (\d+)/.exec(message);
        throw new LocalKimiCredentialError(match
            ? `Kimi Code credentials are not valid JSON (position ${match[1]}). Run /login in Kimi Code.`
            : 'Kimi Code credentials are not valid JSON. Run /login in Kimi Code.');
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
function normalizeUsageResponse(raw) {
    const usages = isRecord(raw) && isRecord(raw.usages) ? raw.usages : {};
    const windows = [];
    for (const [key, label] of WINDOW_KEYS) {
        const entry = usages[key];
        if (!isRecord(entry))
            continue;
        const ratio = typeof entry.used_ratio === 'number'
            ? entry.used_ratio
            : typeof entry.used_ratio === 'string'
                ? parseRatioString(entry.used_ratio)
                : Number.NaN;
        if (!Number.isFinite(ratio))
            continue;
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
function kimiUsageErrorMessage(status) {
    if (status === 401 || status === 403) {
        return 'Kimi Code rejected the stored token. Run Kimi Code to refresh it, or use /login.';
    }
    return `Kimi Code usage request failed with HTTP ${status}.`;
}
//# sourceMappingURL=kimiUsage.js.map