# 闪记

闪记是一款离线优先的跨平台快速备忘与提醒工具。当前仓库处于 MVP 实现阶段。

## 设计目标

- 在任意应用中通过全局快捷键立即开始输入。
- 提交成功后可靠落盘，再隐藏快速录入窗口。
- 无明确时间的事项使用可配置的工作日默认时间。
- “今日必做”事项下班后继续按设置间隔提醒，直到完成或暂停。
- 核心能力不依赖账号或网络。

产品规则见 [docs/PRD.md](docs/PRD.md)，工程约束见 [AGENTS.md](AGENTS.md)。

## 当前已实现

- Tauri 2 主窗口、快速录入窗口、托盘、单实例和全局组合快捷键。
- Rust + SQLite 本地事务保存、草稿恢复、分类与设置。
- 可配置工作日和默认到期时间；已有事项需显式确认才会批量更新。
- “今日必做”持续提醒、应用内勿扰、暂停/恢复、完成即停止。
- 今日必做在默认时间已过时立即提醒；逾期和未完成事项可直接调整时间。
- 后台持久化调度、休眠/重启后恢复、自动顺延和提醒幂等。
- Windows 原生系统横幅、可核验的通知提交状态、失败退避重试和通知点击定位。
- 安装版默认注册原生系统通知；便携版使用当前用户级身份，须在后台设置中明确启用，并可随时撤销。
- 版本化首次启动引导依次确认通知、开机启动、默认提醒时间和全局快捷键，也可从后台设置重新打开。
- 可选开机启动，启动项仅在用户完成引导或保存设置后写入系统；跳过引导不会修改系统状态。
- 快捷键设置会区分格式错误、仅修饰键和系统占用，并在保存失败时保留原快捷键。
- Svelte 管理页、快速录入、设置抽屉、IME 组合态保护和浏览器预览后端。
- Windows NSIS 安装包与 ZIP 便携包。

Windows 已完成真实编译和启动验证。macOS、Linux 共用领域与数据逻辑，并已配置对应系统的自动打包；全局快捷键、托盘、通知和分发仍需在真实设备验证。

## 开发环境

- Node.js 20+ 与 pnpm。
- Rust stable，包含 `rustfmt` 和 `clippy`。
- Windows：Microsoft C++ Build Tools（使用 C++ 的桌面开发）与 WebView2。
- macOS/Linux：按 Tauri 2 官方前置依赖安装对应系统库。

Windows 若普通终端找不到 `link.exe`，请从 “Developer PowerShell for VS” 启动，或先运行 Build Tools 的 `vcvars64.bat`。

## 常用命令

```powershell
pnpm install
pnpm dev
pnpm check
pnpm version:check
pnpm test
pnpm build
pnpm tauri dev
```

后台设置页会显示原生通知身份与最近一次投递状态，并提供“10 秒后测试通知”。测试会创建一条真实事项，走完整 SQLite → 调度器 → 系统通知链路。开发时也可直接运行：

```powershell
src-tauri\target\release\shanji.exe --test-notification
```

Rust 检查：

```powershell
Set-Location src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings -A linker_messages
cargo test
```

生成 Windows NSIS 安装包和便携 ZIP：

```powershell
pnpm tauri build --bundles nsis
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/package-portable.ps1
```

产物默认位于：

- `src-tauri/target/release/bundle/nsis/`
- `artifacts/`

## 跨平台测试包

仓库的“跨平台测试构建”工作流会在对应系统生成：

- Windows x64：NSIS 安装包、ZIP 便携包；
- macOS：兼容 Apple Silicon 与 Intel 的通用 DMG；
- Linux x64：AppImage、DEB。

手动运行工作流只生成可下载的临时构建产物；推送 `v*` 标签时才创建草稿预发布，并附带 `SHA256SUMS.txt`。未配置 Apple Developer 和 Windows 代码签名凭据时，安装过程可能出现系统安全提示。这些包只用于测试，需在对应真实设备完成验证后再转为正式发布。

## 本地数据

普通模式遵循系统应用数据目录；Windows 当前路径为 `%APPDATA%\com.shanji.desktop\shanji.db`。

便携包内含 `portable.flag`。存在该标记时，数据写入程序旁的 `data/shanji.db`。请完整移动或备份整个便携目录，不要只复制正在写入的数据库文件。

便携版启用开机启动后，系统启动项会指向便携程序的当前位置。移动或重命名便携目录后，应在新位置重新关闭并开启该选项，以更新启动路径。
