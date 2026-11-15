# Week 3 / M1 在 Mac 上核对

这些步骤要在 macOS 13 或更高版本上执行。Linux 上的 `pnpm dev` 只能看壳的布局，没有分区 WebView。

```bash
pnpm install
pnpm tauri dev
```

退出时点窗口左上角的红色关闭按钮，确认进程已经结束，再执行下一次 `pnpm tauri dev`。不要只关掉终端却把应用留在后台。

## 登录态跨重启

每个已确认网页，以及每一个手动添加的链接，各有一个持久分区。cookie 和 localStorage 写在磁盘上，退出再打开还会在。分区之间不共用这份数据。

1. 侧栏点 ChatGPT，在里面登录。
2. 内容区切到 1:1。点右侧空格，再点 Claude，也登录。
3. 再切到田字格，把豆包、WorkBuddy 或 DeepSeek 放进另外的格子。
4. 用红色关闭按钮退出。
5. 再次 `pnpm tauri dev`。
6. 布局、每一格里的站点应和退出前一样，而且不需要重新登录。
7. 退出其中一个站点的登录，另一个格子里的登录应保持不变。

配置文件是 `~/Library/Application Support/com.uniworkspace.ai/workspace.json`。打开过网页之后，同目录下的 `partitions/index.json` 会列出每个分区的 UUID。

- macOS 14 及以上：`~/Library/WebKit/com.uniworkspace.ai/WebsiteDataStore/<uuid>/`
- macOS 13：`~/Library/Application Support/com.uniworkspace.ai/partitions/<应用 id>/`

Week 2 的四个 UUID 没有变。豆包、WorkBuddy、DeepSeek 和自定义链接只是按同一个规则多出来的分区，不是内存仓库。

## 待确认的站点

Accio Work、Cursor、Harness 在侧栏里标着「不打开」。点它们只会说明没有稳定的官方网页入口，内容区不会出现新的 WebView。不要为了试这些条目去改 `catalog/apps.json` 里的空网址或空 Bundle ID。

## 侧栏和自定义链接

把窗口拉矮，或把侧栏里的网页、自定义链接和桌面条目一起看。列表放不下时应在侧栏内部滚动，标题栏和「添加到独立分区」仍留在原处。

在底部输入一个 `https://` 链接并添加。它占用自己的 `custom-<uuid>` 分区，可以放进当前格子。退出再打开后，这个链接还在同一格，并且仍然不和其他站点共用 cookie。

## 隔离

侧栏底部的「检查隔离」会在全部已确认站点的仓库里打开同一个 `127.0.0.1` 页面，分别写入 cookie 和 localStorage，再读回来。任何一个分区读到别人的值就未通过。这包含 Week 2 的 ChatGPT、Claude、Gemini、Kimi，也包含豆包、WorkBuddy 和 DeepSeek。

## 单个 WebView 崩溃

打开 1:1，两格放两个不同的已登录站点。

活动监视器里会有这个应用的 Web Content 进程。结束其中一个网页内容进程，而不是结束 UniWorkspace AI 主进程。只有对应那一格会按原分区重新打开；另一格应停在原来的页面，不重新加载，登录也不丢。

如果某一格连续崩溃，重建会停下来，格子上会写明需要从侧栏再点一次。这不会关掉其他格子。

壳或登录弹窗自己的内容进程结束时，只重新载入那一个 WebView。
