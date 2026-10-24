// @env node
import * as vscode from 'vscode';

import { PROVIDER_LABELS } from './constants';
import {
  kimiUsageErrorMessage,
  normalizeUsageResponse,
  readLocalCredentials,
  type KimiUsageSummary,
  type KimiUsageWindow,
} from './kimiUsage';
import type { QuotaTrack, TrackId } from './types';

const KIMI_USAGE_URL = 'https://api.kimi.com/coding/v1/usages';
const KIMI_ACCOUNTS_KEY = 'quota.kimi.accounts';
const KIMI_ACCOUNT_ID = 'kimi_local';
const KIMI_ACCOUNT_LABEL = 'Kimi Code';
const REQUEST_TIMEOUT_MS = 8_000;

interface KimiAccount {
  id: string;
  label: string;
  usage: KimiUsageSummary;
  quotaQueryLastError?: string | null;
  quotaQueryLastErrorAt?: number | null;
  usageUpdatedAt?: number | null;
  createdAt: number;
  lastUsed: number;
}

const WINDOW_TRACKS: Array<[TrackId, string, string]> = [
  ['kimi.fiveHour', '5h', '5h usage'],
  ['kimi.weekly', 'Wk', 'Weekly usage'],
  ['kimi.monthly', 'Month', 'Monthly usage'],
  ['kimi.monthlyCode', 'Code', 'Monthly Code usage'],
];

function track(account: KimiAccount, id: TrackId, label: string, window: KimiUsageWindow | undefined): QuotaTrack {
  return {
    id,
    providerId: 'kimi',
    providerLabel: PROVIDER_LABELS.kimi,
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

function tracksFromAccount(account: KimiAccount): QuotaTrack[] {
  return WINDOW_TRACKS.map(([id, windowLabel, label]) =>
    track(account, id, label, account.usage.windows.find((window) => window.label === windowLabel)),
  );
}

export class KimiProvider {
  constructor(private readonly context: vscode.ExtensionContext) {}

  /** Import the local Kimi Code login. Importing again reconnects the single account. */
  async connect(): Promise<KimiAccount | undefined> {
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

  async refreshAll(): Promise<KimiAccount[]> {
    const accounts = await this.getAccounts();
    const refreshed: KimiAccount[] = [];
    for (const account of accounts) refreshed.push(await this.refreshAccount(account.id));
    return refreshed;
  }

  async refreshAccount(accountId: string): Promise<KimiAccount> {
    const account = await this.getAccount(accountId);
    if (!account) throw new Error('Kimi Code account is not connected.');

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
    } catch (error) {
      // Keep the last successful usage and only record what went wrong.
      const message = error instanceof Error ? error.message : String(error);
      return this.saveAccount({ ...account, quotaQueryLastError: message, quotaQueryLastErrorAt: Date.now() });
    }
  }

  async disconnect(): Promise<void> {
    const accounts = await this.getAccounts();
    if (accounts.length === 0) {
      void vscode.window.showInformationMessage('No Kimi Code account is connected.');
      return;
    }

    const account = accounts[0];
    const confirmed = await vscode.window.showWarningMessage(
      `Disconnect Kimi Code? Quota's cached usage summary will be deleted. Kimi Code's own credentials are not touched.`,
      { modal: true },
      'Disconnect',
    );
    if (confirmed !== 'Disconnect') return;

    await this.context.globalState.update(
      KIMI_ACCOUNTS_KEY,
      accounts.filter((item) => item.id !== account.id),
    );
  }

  async getTracks(): Promise<QuotaTrack[]> {
    const accounts = await this.getAccounts();
    return accounts
      .flatMap(tracksFromAccount)
      .filter((item) => item.percentUsed != null || item.error);
  }

  async hasAccounts(): Promise<boolean> {
    return (await this.getAccounts()).length > 0;
  }

  private async fetchUsage(): Promise<KimiUsageSummary> {
    // Read the local credentials on every refresh: the token rotates with Kimi
    // Code, and Quota never stores or renews it.
    const credentials = readLocalCredentials();

    let response: Response;
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
    } catch {
      throw new Error('Could not reach Kimi Code. Check your connection and try again.');
    }

    if (!response.ok) throw new Error(kimiUsageErrorMessage(response.status));

    let body: unknown;
    try {
      body = await response.json();
    } catch {
      throw new Error('Kimi Code usage response was not valid JSON.');
    }
    return normalizeUsageResponse(body);
  }

  private async getAccounts(): Promise<KimiAccount[]> {
    return this.context.globalState.get<KimiAccount[]>(KIMI_ACCOUNTS_KEY, []);
  }

  private async getAccount(accountId?: string): Promise<KimiAccount | undefined> {
    const id = accountId ?? KIMI_ACCOUNT_ID;
    return (await this.getAccounts()).find((account) => account.id === id);
  }

  private async saveAccount(account: KimiAccount): Promise<KimiAccount> {
    const accounts = await this.getAccounts();
    const next = [account, ...accounts.filter((item) => item.id !== account.id)];
    await this.context.globalState.update(KIMI_ACCOUNTS_KEY, next);
    return account;
  }
}
