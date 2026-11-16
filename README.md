# UniWorkspace AI

个人使用的本地 macOS AI 工作台。Week 3 把已确认的网页放进可恢复的分屏：侧栏切换、单窗 / 1:1 / 田字格，登录态按应用隔开并在重启后还在。不调用第三方 AI 接口，也不迁移各家客户端里已经登录的数据。

最低系统版本是 macOS 13。

在 Mac 上核对登录、分屏和单个 WebView 崩溃，按 `docs/week3-m1.md`。

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

已确认的网页各用一个持久的 WKWebsiteDataStore，cookie 和 localStorage 不共享。macOS 14 及以上用 `dataStoreForIdentifier`；macOS 13 没有这套公开 API，改用按应用目录配置的持久仓库，不用内存仓库。决定写在 `docs/week2-partition-decision.md`。Week 3 沿用同一套标识，没有把新站点放进内存仓库。

会打开的官方地址：

- ChatGPT `https://chatgpt.com/`
- Claude `https://claude.ai/`
- Gemini `https://gemini.google.com/`
- Kimi `https://www.kimi.com/`
- 豆包 `https://www.doubao.com/chat/`
- WorkBuddy `https://www.workbuddy.cn/app`
- DeepSeek `https://chat.deepseek.com/`

Accio Work、Cursor、Harness 没有稳定的官方网页入口，侧栏标成「不打开」，不会猜地址。WorkBuddy、Accio Work、Harness 的桌面 Bundle ID 仍是空的。

手动添加的链接使用 `custom-<uuid>` 分区，和目录里的站点分开，并且写进同一份本地配置。

登录用的 `window.open` 会开一个独立窗口，并沿用当前分区的数据仓库。

在 Mac 上检查这些分区有没有串数据：运行 `pnpm tauri dev`，点侧栏底部的「检查隔离」。它会在每个已确认站点的仓库里打开同一个 `127.0.0.1` 页面，分别写入 cookie 和 localStorage，再读回来比对。浏览器里的 `pnpm dev` 没有这些 WebView。

## 布局

内容区可以是单窗、左右 1:1，或田字格。点一格，再点侧栏里的网页，就把那个网页放进这一格。同一个网页不会同时出现在两格。

布局、每一格里的应用，以及自定义链接写在：

`~/Library/Application Support/com.uniworkspace.ai/workspace.json`

退出时用窗口左上角的关闭按钮，让进程结束。再次 `pnpm tauri dev` 会读回这份文件，并用原来的分区打开原来的网页，所以登录态还在。步骤见 `docs/week3-m1.md`。

## 单个 WebView 崩溃

某个分区的网页内容进程结束时，只关闭并按原来的分区、网址和位置重建那一格。其他格子不关闭、不重新加载。壳本身或登录弹窗如果崩溃，只重新载入那一个 WebView。

Tauri 2.12 的 `WebviewBuilder` 还没有单独的崩溃回调，应用级的 `on_web_content_process_terminate` 会带上崩溃的那个 WebView。恢复按这个 WebView 的标签决定，不缩小已确认站点的范围。

## 本周范围

`src/` 是 React 壳。`detector` 和 `shortcuts` 仍是占位。`window_mac` 只保留以后做吸附的注释，没有调用辅助功能，也没有开始几何吸附、热键或公证。

本周不包含 Windows、Linux，也不包含给 Accio Work、Cursor、Harness 编造网页地址。
