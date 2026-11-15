# UniWorkspace AI

个人使用的本地 macOS AI 工作台。Week 1 只交付空壳：自定义标题栏、侧栏占位和内容区。不调用第三方 AI 接口，也不迁移已有登录数据。

最低系统版本是 macOS 13。

## 在 Mac 上开发

需要：

- macOS 13 或更高
- Xcode Command Line Tools
- Rust stable（Tauri 2.12 需要 1.90 或更高）
- Node.js 22
- pnpm 10

```bash
pnpm install
pnpm tauri dev
```

只看前端时可以运行 `pnpm dev`，然后打开 `http://127.0.0.1:1420`。浏览器里没有 Tauri 窗口，标题栏按钮不会真正关闭或缩放窗口。

## CI

`.github/workflows/ci.yml` 只在 macOS 上运行。它检查 `catalog/apps.json`，然后执行：

```bash
pnpm tauri build --debug --bundles app --no-sign
```

产物是未签名的 debug `.app`。CI 先用 `ditto` 把它打成 zip 再上传，这样下载解压后主程序仍保留可执行权限。工作流里没有 Windows 或 Linux 任务。

## 应用目录

`catalog/apps.json` 是静态目录，列出 10 个网页应用，以及已核对过的 macOS 桌面客户端。`webUrl` 或 `bundleId` 还不能确认时保持为 `null`，说明写在 `notes` 里。不要把备注中的候选值当成已经核对过的标识。

本地检查：

```bash
pnpm catalog:check
```

## 分区

Week 2 在内容区打开目录里的 ChatGPT、Claude、Gemini 和 Kimi。每个应用一个持久的 WKWebsiteDataStore，cookie 和 localStorage 不共享。macOS 14 及以上用 `dataStoreForIdentifier`；macOS 13 没有这套公开 API，改用按应用目录配置的持久仓库，不用内存仓库。决定写在 `docs/week2-partition-decision.md`。

登录用的 `window.open` 会开一个独立窗口，并沿用当前分区的数据仓库。

其余网页和全部桌面应用仍然只标出位置。辅助功能和吸附只做了调研，见 `docs/week2-a11y-placeholder-research.md`，没有跟随窗口。

在 Mac 上检查四个分区有没有串数据：运行 `pnpm tauri dev`，点侧栏底部的「检查隔离」。它会在四个仓库里打开同一个 `127.0.0.1` 页面，分别写入 cookie 和 localStorage，再读回来比对。浏览器里的 `pnpm dev` 没有这些 WebView。

## 本周范围

`src/` 是 React 壳，内容区可以盖上上述四个分区 WebView。`detector` 和 `shortcuts` 仍是占位。`window_mac` 只加了以后做吸附的注释，没有调用辅助功能。

本周不包含其余六个网页、分屏、重启后的登录恢复界面、窗口吸附、全局热键，或其他平台的窗口嵌入。
