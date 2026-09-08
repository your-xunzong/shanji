import { isTauri } from '@tauri-apps/api/core';
import type { DownloadEvent, Update } from '@tauri-apps/plugin-updater';

const RELEASE_ASSET_PREFIX = 'https://github.com/your-xunzong/shanji/releases/download/';

export interface AppUpdate {
  currentVersion: string;
  version: string;
  date: string | null;
  body: string;
  notes: string[];
}

export interface UpdateDownloadProgress {
  downloadedBytes: number;
  totalBytes: number | null;
  percent: number | null;
  finished: boolean;
}

export type UpdatePhase =
  | 'checking'
  | 'available'
  | 'downloading'
  | 'downloaded'
  | 'installing'
  | 'upToDate'
  | 'failed'
  | 'cancelled'
  | 'current';

let pendingUpdate: Update | null = null;
let downloaded = false;
let cancelRequested = false;

export function parseUserFacingNotes(body: string | undefined): string[] {
  if (!body) return ['此版本包含稳定性与使用体验改进。'];
  const bullets = body
    .split(/\r?\n/)
    .map((line) => line.match(/^\s*[-*]\s+(.+?)\s*$/)?.[1]?.trim())
    .filter((line): line is string => Boolean(line));
  return bullets.length > 0 ? bullets.slice(0, 6) : [body.trim().slice(0, 240)];
}

function releaseUrls(rawJson: Record<string, unknown>): string[] {
  const urls: string[] = [];
  const visit = (value: unknown): void => {
    if (!value || typeof value !== 'object') return;
    for (const [key, child] of Object.entries(value)) {
      if (key === 'url' && typeof child === 'string') urls.push(child);
      else visit(child);
    }
  };
  visit(rawJson);
  return urls;
}

export function isTrustedReleaseJson(rawJson: Record<string, unknown>): boolean {
  const urls = releaseUrls(rawJson);
  return urls.length > 0 && urls.every((url) => url.startsWith(RELEASE_ASSET_PREFIX));
}

function assertTrustedRelease(update: Update): void {
  if (!isTrustedReleaseJson(update.rawJson)) {
    throw new Error('更新来源无法验证，当前版本和本机数据未改变。');
  }
}

export async function checkForAppUpdate(): Promise<AppUpdate | null> {
  if (!isTauri()) return null;
  const { check } = await import('@tauri-apps/plugin-updater');
  if (pendingUpdate) {
    await pendingUpdate.close().catch(() => undefined);
    pendingUpdate = null;
  }
  downloaded = false;
  cancelRequested = false;
  const update = await check({ timeout: 15_000 });
  if (!update) return null;
  assertTrustedRelease(update);
  pendingUpdate = update;
  return {
    currentVersion: update.currentVersion,
    version: update.version,
    date: update.date ?? null,
    body: update.body ?? '',
    notes: parseUserFacingNotes(update.body),
  };
}

export async function downloadAppUpdate(
  onProgress: (progress: UpdateDownloadProgress) => void,
): Promise<void> {
  const update = pendingUpdate;
  if (!update) throw new Error('请重新检查更新后再下载。');
  let downloadedBytes = 0;
  let totalBytes: number | null = null;
  cancelRequested = false;

  const report = (event: DownloadEvent): void => {
    if (event.event === 'Started') totalBytes = event.data.contentLength ?? null;
    if (event.event === 'Progress') downloadedBytes += event.data.chunkLength;
    onProgress({
      downloadedBytes,
      totalBytes,
      percent: totalBytes && totalBytes > 0 ? Math.min(100, downloadedBytes / totalBytes * 100) : null,
      finished: event.event === 'Finished',
    });
  };

  try {
    await update.download(report, { timeout: 10 * 60 * 1000 });
    if (cancelRequested) throw new Error('下载已取消，当前版本没有改变。');
    downloaded = true;
  } catch (cause) {
    downloaded = false;
    if (cancelRequested) throw new Error('下载已取消，当前版本没有改变。');
    throw cause;
  }
}

export async function cancelAppUpdateDownload(): Promise<void> {
  cancelRequested = true;
  downloaded = false;
  if (pendingUpdate) await pendingUpdate.close().catch(() => undefined);
}

export async function installAppUpdate(): Promise<void> {
  if (!pendingUpdate || !downloaded) throw new Error('更新包还没有下载完成。');
  await pendingUpdate.install();
  const { relaunch } = await import('@tauri-apps/plugin-process');
  await relaunch();
}

export async function clearPendingUpdate(): Promise<void> {
  downloaded = false;
  cancelRequested = false;
  if (pendingUpdate) await pendingUpdate.close().catch(() => undefined);
  pendingUpdate = null;
}
