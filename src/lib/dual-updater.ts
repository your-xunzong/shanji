import { Channel, invoke, isTauri } from '@tauri-apps/api/core';

const TRUSTED_RELEASE_PREFIXES = [
  'https://github.com/your-xunzong/shanji/releases/download/',
  'https://gitee.com/wu_-ji/shanji/',
];

export type UpdateSource = 'GITHUB' | 'GITEE';

export interface AppUpdate {
  currentVersion: string;
  version: string;
  date: string | null;
  body: string;
  notes: string[];
  source: UpdateSource;
  latencyMs: number;
  fallbackAvailable: boolean;
  mirrorStatus: 'READY' | 'SINGLE_SOURCE' | 'OUT_OF_SYNC';
}

interface RawAppUpdate extends Omit<AppUpdate, 'notes'> {}

interface BackendDownloadEvent {
  event: 'started' | 'progress' | 'finished';
  chunkLength: number | null;
  contentLength: number | null;
  source: UpdateSource;
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
  return urls.length > 0 && urls.every((url) =>
    TRUSTED_RELEASE_PREFIXES.some((prefix) => url.startsWith(prefix))
  );
}

export async function checkForAppUpdate(): Promise<AppUpdate | null> {
  if (!isTauri()) return null;
  downloaded = false;
  cancelRequested = false;
  const update = await invoke<RawAppUpdate | null>('check_app_update');
  return update ? { ...update, notes: parseUserFacingNotes(update.body) } : null;
}

export async function downloadAppUpdate(
  onProgress: (progress: UpdateDownloadProgress) => void,
): Promise<void> {
  let downloadedBytes = 0;
  let totalBytes: number | null = null;
  cancelRequested = false;
  const onEvent = new Channel<BackendDownloadEvent>((event) => {
    if (event.event === 'started') {
      downloadedBytes = 0;
      totalBytes = event.contentLength;
    } else if (event.event === 'progress') {
      if (event.contentLength !== null) totalBytes = event.contentLength;
      downloadedBytes += event.chunkLength ?? 0;
    }
    onProgress({
      downloadedBytes,
      totalBytes,
      percent: totalBytes && totalBytes > 0 ? Math.min(100, downloadedBytes / totalBytes * 100) : null,
      finished: event.event === 'finished',
    });
  });
  try {
    await invoke<void>('download_app_update', { onEvent });
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
  if (isTauri()) await invoke<void>('cancel_app_update');
}

export async function installAppUpdate(): Promise<void> {
  if (!downloaded) throw new Error('更新包还没有下载完成。');
  await invoke<void>('install_app_update');
  const { relaunch } = await import('@tauri-apps/plugin-process');
  await relaunch();
}

export async function clearPendingUpdate(): Promise<void> {
  downloaded = false;
  cancelRequested = false;
  if (isTauri()) await invoke<void>('clear_pending_app_update');
}
