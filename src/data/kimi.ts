import { invoke } from '@tauri-apps/api/core';

export interface KimiUsageWindow {
  label: string;
  remainingPercent: number | null;
  resetAt: number | null;
}

export interface KimiUsage {
  windows: KimiUsageWindow[];
  note: string | null;
}

export interface KimiAccountSummary {
  id: string;
  label: string;
  usage: KimiUsage;
  quotaQueryLastError: string | null;
  quotaQueryLastErrorAt: number | null;
  usageUpdatedAt: number | null;
  createdAt: number;
  lastUsed: number;
}

export function listKimiAccounts(): Promise<KimiAccountSummary[]> {
  return invoke<KimiAccountSummary[]>('list_kimi_accounts');
}

export function importKimiFromLocal(): Promise<KimiAccountSummary> {
  return invoke<KimiAccountSummary>('import_kimi_from_local');
}

export function refreshKimiAccount(accountId: string): Promise<KimiAccountSummary> {
  return invoke<KimiAccountSummary>('refresh_kimi_account', { accountId });
}

export function refreshAllKimiAccounts(): Promise<KimiAccountSummary[]> {
  return invoke<KimiAccountSummary[]>('refresh_all_kimi_accounts');
}

export function deleteKimiAccount(accountId: string): Promise<void> {
  return invoke<void>('delete_kimi_account', { accountId });
}
