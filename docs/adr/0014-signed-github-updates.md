# ADR-0014：签名 GitHub Release 更新与设备级检查状态

- 状态：已接受；2026-09-08，v0.12.0

采用 Tauri 2 官方 updater 与 process 插件。生产端点固定为仓库稳定 Release 的 `latest.json`，客户端除插件的强制 Minisign 校验外，再限制更新包 URL 必须位于 `https://github.com/your-xunzong/shanji/releases/download/`。不增加自建更新服务器、远程 HTML 渲染或运行时可编辑端点。

更新公钥编译进 `tauri.conf.json`，长期私钥保存在仓库外并通过 `TAURI_SIGNING_PRIVATE_KEY` 注入 GitHub Actions；私钥不进入源码、SQLite、日志或附件。`bundle.createUpdaterArtifacts` 生成各平台更新包和 `.sig`。标签流水线汇集普通安装包、更新包与签名，用源码中的单份中文说明生成 `latest.json` 和稳定版草稿；维护者完成真实验收并在 GitHub 发布草稿后，客户端才会通过 `/releases/latest/` 发现它。

自动检查默认关闭。SQLite schema v11 新增独立的 `update_state` 单例表，持久化用户授权、上次检查、稍后提醒、已通知版本与已读说明。该表不是事项事实，不进入个人仓库白名单或跨设备归并；检查间隔由 Rust 使用 UTC 判断，未来时间不会触发补查，避免休眠、重启和时钟回拨造成请求风暴。前端只在启动 30 秒后按后端决策调用 updater，手动检查则是一次明确联网授权。

选择官方插件会增加 HTTP、归档和签名验证依赖及少量包体积，但不引入常驻服务或轮询。网络完全离开快速录入、保存和提醒调度路径；下载与安装只能由用户操作触发，错误转换为“当前版本和数据未改变”的恢复文案。更新说明只解析 3–6 条纯文本列表，Svelte 默认转义远程内容。

Windows NSIS 使用被动安装；便携版只打开 Release。macOS 生成通用 `.app.tar.gz`，在签名、公证和实机重启验证完成前不宣称通过。Linux 仅在 AppImage 环境允许应用内安装，`.deb` 与其他包管理器环境打开 Release。v0.12.0 是首个带公钥版本，必须手动安装；端到端更新需由后续稳定版本验证。
