# ADR-0001：采用 Tauri 2、Rust、Svelte 与 SQLite

- 状态：Accepted
- 日期：2026-08-27

## 背景

产品要求快速全局唤起、后台调度、本地数据、系统通知、低资源占用和 Windows/macOS/Linux 支持。

## 决策

- 使用 Tauri 2 作为桌面壳与系统能力边界。
- 使用 Rust 实现领域规则、SQLite 数据访问、提醒调度和平台适配。
- 使用 TypeScript + Svelte + Vite 实现 SPA 界面。
- 使用 SQLite 作为本地事实来源，UI 不直接访问数据库。

## 影响

- 优点：使用系统 WebView，前端资源较小；Rust 可承载可靠的时间和持久化规则；三平台共享大部分实现。
- 代价：Windows 开发需要 Rust、MSVC Build Tools 和 WebView2；Linux 的托盘与全局快捷键需要按 Wayland/X11 分别验证；macOS 分发需要签名与公证。
- 约束：平台 API 必须置于适配层；依赖和 capabilities 采用最小集合；技术栈变更需新 ADR。
