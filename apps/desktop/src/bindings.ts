import { invoke } from '@tauri-apps/api/core';

export interface AppStatus {
  appVersion: string;
  schemaVersion: number;
  dataDir: string;
}

export async function appStatus(): Promise<AppStatus> {
  return invoke<AppStatus>('app_status');
}
