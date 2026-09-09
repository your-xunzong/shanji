import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { basename, resolve } from "node:path";

// 打包器复制语言文件时会补 UTF-8 BOM，Windows 检出也可能转换换行。
const normalizeText = (value) => value.replace(/^\uFEFF/u, "").replaceAll("\r\n", "\n");

const config = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const nsis = config.bundle?.windows?.nsis;
assert.deepEqual(nsis?.languages, ["SimpChinese"], "Windows 安装器必须只启用简体中文。");
assert.equal(nsis.displayLanguageSelector, false, "安装时不得增加语言选择步骤。");
const localePath = nsis.customLanguageFiles?.SimpChinese;
assert.ok(localePath, "缺少安装器专用中文文案。");
const locale = readFileSync(resolve("src-tauri", localePath), "utf8");
assert.match(config.bundle.copyright ?? "", /闪记/u, "向导底部应使用闪记版权信息，避免回退到英文框架标识。");
const entries = [...locale.matchAll(/^LangString\s+(\w+)\s+\$\{LANG_SIMPCHINESE\}\s+"(.+)"\s*$/gm)];
const keys = new Set(entries.map((entry) => entry[1]));
const required = [
  "addOrReinstall", "alreadyInstalled", "alreadyInstalledLong", "appRunning",
  "appRunningOkKill", "chooseMaintenanceOption", "choowHowToInstall", "createDesktop",
  "dontUninstall", "dontUninstallDowngrade", "failedToKillApp", "installingWebview2",
  "newerVersionInstalled", "older", "olderOrUnknownVersionInstalled", "silentDowngrades",
  "unableToUninstall", "uninstallApp", "uninstallBeforeInstalling", "unknown",
  "webview2AbortError", "webview2DownloadError", "webview2DownloadSuccess",
  "webview2Downloading", "webview2InstallError", "webview2InstallSuccess", "deleteAppData",
];
for (const key of required) assert.ok(keys.has(key), `缺少中文文案：${key}`);
assert.equal(keys.size, entries.length, "中文文案标识不得重复。");
assert.equal((locale.match(/^LangString\s/gm) ?? []).length, entries.length, "语言声明或文案格式不正确。");
for (const [, key, value] of entries) {
  assert.match(value, /[\u3400-\u9fff]/u, `${key} 必须包含中文。`);
  assert.doesNotMatch(value, /\{\{|\}\}/u, `${key} 含未替换的模板变量。`);
}

if (process.argv.includes("--generated")) {
  const script = readFileSync("src-tauri/target/release/nsis/x64/installer.nsi", "utf8");
  const languages = [...script.matchAll(/^\s*!insertmacro MUI_LANGUAGE "([^"]+)"/gm)].map((entry) => entry[1]);
  assert.deepEqual(languages, ["SimpChinese"], "生成的安装脚本未使用简体中文。");
  assert.ok(script.includes(`!define VERSION "${config.version}"`), "安装脚本不是当前版本。");
  assert.ok(script.includes(`!define COPYRIGHT "${config.bundle.copyright}"`), "生成脚本未使用闪记版权信息。");
  const includes = [...script.matchAll(/^\s*!include "([^"]+)"/gm)].map((entry) => entry[1]);
  const generatedLocalePath = includes.find((path) => basename(path) === "SimpChinese.nsh");
  assert.ok(generatedLocalePath, "生成脚本未包含自定义中文文案。");
  assert.equal(normalizeText(readFileSync(generatedLocalePath, "utf8")), normalizeText(locale), "打包使用的中文文案不是当前文件。");
  // 依赖升级新增安装器文案时也要拦截，避免只验证当前已知文案。
  for (const [, key] of script.matchAll(/\$\(([a-z]\w*)\)/g)) {
    assert.ok(keys.has(key), `生成脚本使用了未翻译的文案：${key}`);
  }
  console.log(`生成的 ${config.version} Windows 安装/卸载脚本中文检查通过。`);
}
console.log(`安装器语言配置与 ${entries.length} 条中文文案检查通过。`);
