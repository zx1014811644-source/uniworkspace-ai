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

## 本周范围

`src/` 是 React 壳。`src-tauri/src/` 里的 `detector`、`window_mac`、`shortcuts` 只是能编译的占位模块。

本周不包含多分 WebView、登录保持、分屏、窗口吸附、全局热键，或其他平台的窗口嵌入。
