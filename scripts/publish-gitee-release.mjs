import { spawnSync } from 'node:child_process';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';

const API_ROOT = 'https://gitee.com/api/v5';
const MAX_ASSET_BYTES = 100 * 1024 * 1024;

function argsMap(values) {
  const result = new Map();
  for (let index = 0; index < values.length; index += 2) {
    const key = values[index];
    const value = values[index + 1];
    if (!key?.startsWith('--') || value === undefined) throw new Error(`Invalid argument near ${key ?? 'end'}.`);
    result.set(key.slice(2), value);
  }
  return result;
}

function safe(value, pattern, label) {
  if (!pattern.test(value ?? '')) throw new Error(`Invalid ${label}.`);
  return value;
}

async function request(path, token, options = {}, allowNotFound = false) {
  const headers = new Headers(options.headers);
  headers.set('Accept', 'application/json');
  headers.set('Authorization', `token ${token}`);
  const response = await fetch(`${API_ROOT}${path}`, { ...options, headers });
  if (allowNotFound && response.status === 404) return null;
  if (!response.ok) {
    const detail = (await response.text()).replace(/\s+/g, ' ').trim().slice(0, 240);
    throw new Error(`Gitee API request failed (${response.status})${detail ? `: ${detail}` : '.'}`);
  }
  return response.status === 204 ? null : response.json();
}

function releaseForm(tag, name, body, prerelease) {
  const form = new URLSearchParams();
  form.set('tag_name', tag);
  form.set('name', name);
  form.set('body', body);
  form.set('target_commitish', 'main');
  form.set('prerelease', prerelease ? 'true' : 'false');
  return form;
}

async function upsertRelease(owner, repo, token, tag, name, body, prerelease) {
  const encodedTag = encodeURIComponent(tag);
  const existing = await request(`/repos/${owner}/${repo}/releases/tags/${encodedTag}`, token, {}, true);
  const form = releaseForm(tag, name, body, prerelease);
  if (!existing) {
    return request(`/repos/${owner}/${repo}/releases`, token, { method: 'POST', body: form });
  }
  return request(`/repos/${owner}/${repo}/releases/${existing.id}`, token, { method: 'PATCH', body: form });
}

async function listReleaseAssets(owner, repo, token, releaseId) {
  const response = await request(`/repos/${owner}/${repo}/releases/${releaseId}/attach_files`, token);
  if (Array.isArray(response)) return response;
  return response?.attach_files ?? response?.assets ?? [];
}

async function replaceAsset(owner, repo, token, release, path) {
  const name = basename(path);
  if (!/^[0-9A-Za-z._-]+$/.test(name)) throw new Error(`Release asset name is not portable: ${name}`);
  const size = statSync(path).size;
  if (size > MAX_ASSET_BYTES) throw new Error(`Gitee asset exceeds 100 MB: ${name}`);
  const prior = (await listReleaseAssets(owner, repo, token, release.id)).filter((asset) => asset.name === name);
  for (const asset of prior) {
    if (!asset.id) throw new Error(`Gitee release asset is missing its deletion id: ${name}`);
    await request(`/repos/${owner}/${repo}/releases/${release.id}/attach_files/${asset.id}`, token, { method: 'DELETE' });
  }
  const url = `${API_ROOT}/repos/${owner}/${repo}/releases/${release.id}/attach_files`;
  const config = [
    'silent',
    'show-error',
    'fail-with-body',
    'connect-timeout = 30',
    'max-time = 1800',
    'header = "Accept: application/json"',
    `header = "Authorization: token ${token}"`,
    `form = "access_token=${token}"`,
  ].join('\n');
  console.log(`Uploading Gitee release asset: ${name} (${size} bytes).`);
  const result = spawnSync(
    'curl',
    ['--config', '-', '--request', 'POST', '--form', `file=@${path};filename=${name}`, '--url', url],
    { input: config, encoding: 'utf8', maxBuffer: 1024 * 1024 },
  );
  if (result.status !== 0) {
    const detail = `${result.stderr ?? ''} ${result.stdout ?? ''}`.replace(/\s+/g, ' ').trim().slice(0, 240);
    throw new Error(`Gitee asset upload failed for ${name}${detail ? `: ${detail}` : '.'}`);
  }
  console.log(`Uploaded Gitee release asset: ${name}.`);
}

async function pruneOldStableAssets(owner, repo, token, keep) {
  const releases = await request(`/repos/${owner}/${repo}/releases?per_page=100`, token);
  const old = releases
    .filter((release) => !release.prerelease)
    .sort((left, right) => Date.parse(right.created_at) - Date.parse(left.created_at))
    .slice(keep);
  for (const release of old) {
    for (const asset of await listReleaseAssets(owner, repo, token, release.id)) {
      await request(`/repos/${owner}/${repo}/releases/${release.id}/attach_files/${asset.id}`, token, { method: 'DELETE' });
    }
  }
}

async function verifyRelease(owner, repo, token, tag, prerelease, assets) {
  const release = await request(
    `/repos/${owner}/${repo}/releases/tags/${encodeURIComponent(tag)}`,
    token,
  );
  if (Boolean(release.prerelease) !== prerelease) {
    throw new Error('Gitee release state does not match the requested state.');
  }
  const attachments = await listReleaseAssets(owner, repo, token, release.id);
  for (const path of assets) {
    const name = basename(path);
    const attachment = attachments.find((value) => value.name === name);
    if (!attachment) throw new Error(`Gitee release is missing asset: ${name}`);
    if (attachment.size !== undefined && Number(attachment.size) !== statSync(path).size) {
      throw new Error(`Gitee release asset size mismatch: ${name}`);
    }
    const downloadUrl = attachment.browser_download_url ?? attachment.browserDownloadUrl ?? '';
    if (!downloadUrl.startsWith(`https://gitee.com/${owner}/${repo}/releases/download/${encodeURIComponent(tag)}/`)) {
      throw new Error(`Gitee release asset URL is outside the official release path: ${name}`);
    }
  }
}

async function main() {
  const args = argsMap(process.argv.slice(2));
  const owner = safe(args.get('owner'), /^[0-9A-Za-z_.-]+$/, 'owner');
  const repo = safe(args.get('repo'), /^[0-9A-Za-z_.-]+$/, 'repository');
  const tag = safe(args.get('tag'), /^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/, 'tag');
  const token = process.env.GITEE_TOKEN;
  if (!token) throw new Error('GITEE_TOKEN is required.');
  if (!/^[0-9A-Za-z_-]+$/.test(token)) throw new Error('GITEE_TOKEN contains unsupported characters.');
  const assetsDirectory = resolve(args.get('assets') ?? 'release-assets');
  const notesPath = resolve(args.get('notes'));
  const prerelease = (args.get('prerelease') ?? 'true') === 'true';
  const notes = readFileSync(notesPath, 'utf8').trim();
  const release = await upsertRelease(owner, repo, token, tag, `闪记 ${tag}`, notes, prerelease);
  const assets = readdirSync(assetsDirectory)
    .map((name) => join(assetsDirectory, name))
    .filter((path) => statSync(path).isFile())
    .sort((left, right) => statSync(left).size - statSync(right).size);
  for (const path of assets) await replaceAsset(owner, repo, token, release, path);
  if (!prerelease) await pruneOldStableAssets(owner, repo, token, Number(args.get('keep') ?? '5'));
  await verifyRelease(owner, repo, token, tag, prerelease, assets);
  console.log(`Gitee release ${tag} synchronized with ${assets.length} assets.`);
}

await main();
