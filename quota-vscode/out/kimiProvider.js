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
exports.KimiProvider = void 0;
// @env node
const vscode = __importStar(require("vscode"));
const constants_1 = require("./constants");
const kimiUsage_1 = require("./kimiUsage");
const KIMI_USAGE_URL = 'https://api.kimi.com/coding/v1/usages';
const KIMI_ACCOUNTS_KEY = 'quota.kimi.accounts';
const KIMI_ACCOUNT_ID = 'kimi_local';
const KIMI_ACCOUNT_LABEL = 'Kimi Code';
const REQUEST_TIMEOUT_MS = 8_000;
const WINDOW_TRACKS = [
    ['kimi.fiveHour', '5h', '5h usage'],
    ['kimi.weekly', 'Wk', 'Weekly usage'],
    ['kimi.monthly', 'Month', 'Monthly usage'],
    ['kimi.monthlyCode', 'Code', 'Monthly Code usage'],
];
function track(account, id, label, window) {
    return {
        id,
        providerId: 'kimi',
        providerLabel: constants_1.PROVIDER_LABELS.kimi,
        label,
        accountId: account.id,
        accountLabel: account.label,
        percentUsed: window?.remainingPercent == null ? undefined : 100 - window.remainingPercent,
        percentRemaining: window?.remainingPercent,
        resetAt: window?.resetAt ?? null,
        updatedAt: account.usageUpdatedAt,
        error: account.quotaQueryLastError ?? null,
    };
}
function tracksFromAccount(account) {
    return WINDOW_TRACKS.map(([id, windowLabel, label]) => track(account, id, label, account.usage.windows.find((window) => window.label === windowLabel)));
}
class KimiProvider {
    context;
    constructor(context) {
        this.context = context;
    }
    /** Import the local Kimi Code login. Importing again reconnects the single account. */
    async connect() {
        // Fails before anything is saved, so a missing or expired session never
        // becomes a half-connected account. The access token is only read from
        // Kimi Code's own credential file and is never copied into Quota state.
        const usage = await this.fetchUsage();
        const existing = await this.getAccount();
        const timestamp = Date.now();
        return this.saveAccount({
            id: KIMI_ACCOUNT_ID,
            label: KIMI_ACCOUNT_LABEL,
            usage,
            quotaQueryLastError: null,
            quotaQueryLastErrorAt: null,
            usageUpdatedAt: timestamp,
            createdAt: existing?.createdAt ?? timestamp,
            lastUsed: timestamp,
        });
    }
    async refreshAll() {
        const accounts = await this.getAccounts();
        const refreshed = [];
        for (const account of accounts)
            refreshed.push(await this.refreshAccount(account.id));
        return refreshed;
    }
    async refreshAccount(accountId) {
        const account = await this.getAccount(accountId);
        if (!account)
            throw new Error('Kimi Code account is not connected.');
        try {
            const usage = await this.fetchUsage();
            const timestamp = Date.now();
            return await this.saveAccount({
                ...account,
                usage,
                quotaQueryLastError: null,
                quotaQueryLastErrorAt: null,
                usageUpdatedAt: timestamp,
                lastUsed: timestamp,
            });
        }
        catch (error) {
            // Keep the last successful usage and only record what went wrong.
            const message = error instanceof Error ? error.message : String(error);
            return this.saveAccount({ ...account, quotaQueryLastError: message, quotaQueryLastErrorAt: Date.now() });
        }
    }
    async disconnect() {
        const accounts = await this.getAccounts();
        if (accounts.length === 0) {
            void vscode.window.showInformationMessage('No Kimi Code account is connected.');
            return;
        }
        const account = accounts[0];
        const confirmed = await vscode.window.showWarningMessage(`Disconnect Kimi Code? Quota's cached usage summary will be deleted. Kimi Code's own credentials are not touched.`, { modal: true }, 'Disconnect');
        if (confirmed !== 'Disconnect')
            return;
        await this.context.globalState.update(KIMI_ACCOUNTS_KEY, accounts.filter((item) => item.id !== account.id));
    }
    async getTracks() {
        const accounts = await this.getAccounts();
        return accounts
            .flatMap(tracksFromAccount)
            .filter((item) => item.percentUsed != null || item.error);
    }
    async hasAccounts() {
        return (await this.getAccounts()).length > 0;
    }
    async fetchUsage() {
        // Read the local credentials on every refresh: the token rotates with Kimi
        // Code, and Quota never stores or renews it.
        const credentials = (0, kimiUsage_1.readLocalCredentials)();
        let response;
        try {
            response = await fetch(KIMI_USAGE_URL, {
                headers: {
                    Authorization: `Bearer ${credentials.accessToken}`,
                    Accept: 'application/json',
                },
                // Never forward the token to a redirect target.
                redirect: 'error',
                signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
            });
        }
        catch {
            throw new Error('Could not reach Kimi Code. Check your connection and try again.');
        }
        if (!response.ok)
            throw new Error((0, kimiUsage_1.kimiUsageErrorMessage)(response.status));
        let body;
        try {
            body = await response.json();
        }
        catch {
            throw new Error('Kimi Code usage response was not valid JSON.');
        }
        return (0, kimiUsage_1.normalizeUsageResponse)(body);
    }
    async getAccounts() {
        return this.context.globalState.get(KIMI_ACCOUNTS_KEY, []);
    }
    async getAccount(accountId) {
        const id = accountId ?? KIMI_ACCOUNT_ID;
        return (await this.getAccounts()).find((account) => account.id === id);
    }
    async saveAccount(account) {
        const accounts = await this.getAccounts();
        const next = [account, ...accounts.filter((item) => item.id !== account.id)];
        await this.context.globalState.update(KIMI_ACCOUNTS_KEY, next);
        return account;
    }
}
exports.KimiProvider = KimiProvider;
//# sourceMappingURL=kimiProvider.js.map