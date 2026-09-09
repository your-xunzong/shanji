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
- 七种固定事件类型，以及用户自定义单选“类型”和彩色多选“标签”的三层整理模型。
- 重复、强调、一次性、强制和自定义提醒方案；重复方案可使用每天两个默认时点或每天一个指定时间。
- 普通、预警、持续、月度和年度事件按各自时间语义排期；重复事项保存自己的时间快照，后台默认值变化不会静默修改旧事项。
- 可配置工作日和默认到期时间；已有事项需显式确认才会批量更新。
- “今日必做”持续提醒、应用内勿扰、暂停/恢复、完成即停止。
- 今日必做在默认时间已过时立即提醒；逾期事项保留原到期时间，需要新计划时通过转换事件类型建立。
- Excel 明细和汇总可分别按事件类型、类型、标签、时间与状态筛选和导出。
- 后台持久化调度、休眠/重启后恢复、自动顺延和提醒幂等。
- Windows 原生系统横幅、可核验的通知提交状态、失败退避重试和通知点击定位。
- 安装版默认注册原生系统通知；便携版使用当前用户级身份，须在后台设置中明确启用，并可随时撤销。
- 版本化首次启动引导依次确认通知、开机启动、默认提醒时间和全局快捷键，也可从后台设置重新打开。
- 可选开机启动，启动项仅在用户完成引导或保存设置后写入系统；跳过引导不会修改系统状态。
- 快捷键设置会区分格式错误、仅修饰键和系统占用，并在保存失败时保留原快捷键。
- Svelte 管理页、快速录入、设置抽屉、IME 组合态保护和浏览器预览后端。
- Windows NSIS 安装包与 ZIP 便携包。
- 更新或重装启动前先只读检查数据；若当前路径为空但找到原数据，会停止进入空列表并提供校验后的恢复入口。
- 个人数据仓库使用带 SHA-256 校验的跨平台 `.sjpack` 数据包；选择位置后立即写入本机快照，归并前检查其他设备的数据，相同修订幂等跳过，分歧内容保留完整冲突副本，运行中的 SQLite 不直接放到同步盘。
- 邮件提醒规则可按事件类型、类型或标签选择首次到期、每天第一次、每个计划时点或每日摘要；连接必须先真实测试，密码只进入系统安全存储，多规则命中同一收件地址会去重。
- 只读事项甘特图支持日、周、月、年范围、今日线和组合筛选；所有事项按创建至截止、完成或今天显示连续状态色段，原截止保留刻痕，未完成逾期以红色斜纹延伸到今天，周期事项仍在同一行分段。
- 屏幕角落提醒窗在正常与空载荷状态均可关闭，支持 `Esc`；关闭只隐藏本次窗口，不会暗中完成、暂停或确认事项。
- 可选的 GitHub 稳定版更新检查、签名验证、中文更新说明、下载进度和安装重启；默认不联网，Windows 便携版与 Linux `.deb` 只提供手动下载入口。
- 中文界面按平台使用微软雅黑 UI、苹方或 Noto Sans CJK，正文和辅助文字采用不低于 12px 的可读字号。

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
pnpm installer:check
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

生成 Windows NSIS 安装包和便携 ZIP 前，需要指定仓库外的更新私钥：

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = "C:\Users\你的用户名\.tauri\shanji-updater.key"
pnpm tauri build --bundles nsis
pnpm installer:check --generated
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/package-portable.ps1
```

Windows 安装与卸载向导使用简体中文，无需选择语言；系统安全提示和第三方运行组件自身界面遵循各自的语言设置。`installer:check` 校验打包配置与专用中文文案，带 `--generated` 时还会核对本次生成的安装脚本。

产物默认位于：

- `src-tauri/target/release/bundle/nsis/`
- `artifacts/`

## 跨平台测试包

仓库的“跨平台测试构建”工作流会在对应系统生成：

- Windows x64：NSIS 安装包、ZIP 便携包；
- macOS：兼容 Apple Silicon 与 Intel 的通用 DMG；
- Linux x64：AppImage、DEB。

手动运行工作流只生成可下载的临时构建产物；推送 `v*` 标签时使用 `TAURI_SIGNING_PRIVATE_KEY` 仓库 Secret 生成更新签名、`latest.json` 和 `SHA256SUMS.txt`，然后创建待确认的稳定版草稿。维护者在 GitHub 正式发布草稿后，已授权的客户端才会发现更新。Windows 发布者签名与 macOS 签名/公证仍是独立门槛，未配置时安装过程可能出现系统安全提示。

## 本地数据

普通模式遵循系统应用数据目录；Windows 当前路径为 `%APPDATA%\com.shanji.desktop\shanji.db`。

覆盖更新、相同版本重装和安装目录变化不会改变普通模式的数据位置。升级前会创建 SQLite 一致性备份并在副本上完成迁移；备份、空间或校验失败时应用停止写入并保留原文件。若出现“找到原来的数据”，请核对记录数量和时间后使用“恢复原数据并重新启动”，不要手动复制正在写入的数据库文件。

便携包内含 `portable.flag`。存在该标记时，数据写入程序旁的 `data/shanji.db`。请完整移动或备份整个便携目录，不要只复制正在写入的数据库文件。

便携版启用开机启动后，系统启动项会指向便携程序的当前位置。移动或重命名便携目录后，应在新位置重新关闭并开启该选项，以更新启动路径。
