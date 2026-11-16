# Week 2 分区实现决定

产品下限仍是 macOS 13。四个站点必须各自拥有持久的网站数据，cookie 和 localStorage 不能串，登录弹窗要能走完。这一周不把壳换成 Electron，也不把系统要求抬到 macOS 14。

## Tauri 2 够不够

Tauri 2.12（wry 0.57）已经能把子 WebView 绑到一个 16 字节的数据仓库标识。wry 在 macOS 14 及以上会调用 `WKWebsiteDataStore.dataStoreForIdentifier:`。这是持久仓库，写在磁盘上，不是 `nonPersistent` 内存仓库。弹窗如果带着系统交给 `createWebViewWithConfiguration` 的同一份 `WKWebViewConfiguration`，就和父页面共用这个仓库。

这套公开 API 在 macOS 13 上不存在。Apple 写明：macOS 14 之前，一个进程里只有一个持久 `WKWebsiteDataStore`，多出来的只能是不落盘的 non-persistent 仓库。wry 在系统版本低于 14 时会退回默认仓库。如果这里跟着退回，四个站点会共享 cookie 和 localStorage，验收失败。如果改用 non-persistent，Week 3 就没法在重启后找回登录。如果把最低系统改成 macOS 14，等于缩小产品范围。

所以：macOS 14 及以上，Tauri 的多分区够用；macOS 13 的公开 API 不够。Week 2 按决策门处理，但不改产品范围，也不拆掉 Week 1 的 Tauri 壳。

## 选定的路径

壳继续是 Tauri 2。内容区里每个目录应用是主窗口的一个子 WKWebView，标签是 `partition-<应用 id>`。

分区名是稳定的 UUID v5，名字是 `https://uniworkspace.ai/partition/<应用 id>`。重启后还是同一个标识，Week 3 可以沿用，不必重做存储。

- macOS 14 及以上：`WebviewBuilder::data_store_identifier`。数据在 `~/Library/WebKit/com.uniworkspace.ai/WebsiteDataStore/<uuid>/`。
- macOS 13：公开 API 做不到第二个持久仓库。这时用 WebKit SPI `_WKWebsiteDataStoreConfiguration`，按应用把 cookie 文件、localStorage、IndexedDB 指到 `~/Library/Application Support/com.uniworkspace.ai/partitions/<应用 id>/`，再放进 `WKWebViewConfiguration`。wry 见到已有 configuration 会保留上面的 `websiteDataStore`，不会换成默认仓库。创建结果如果不是 persistent，打开失败，不会静默退回共享仓库或内存仓库。

这是 Ferdium `persist:<service>` 的分区模型：每个应用一个命名的持久会话，弹窗继承父会话。引擎仍是 WKWebView，因为 CI 必须继续是 `pnpm tauri build --debug --bundles app --no-sign`，而且 Week 1 的壳已经是 Tauri。把整个应用换成 Electron 会丢掉这条构建，也会把已经落地的壳重做一遍，四个站点的隔离并不需要那样做。

打开的网址只来自目录里这四条，Rust 侧再对一次，不一致就拒绝：

- ChatGPT `https://chatgpt.com/`
- Claude `https://claude.ai/`
- Gemini `https://gemini.google.com/`
- Kimi `https://www.kimi.com/`

豆包、WorkBuddy、Accio Work、Cursor、DeepSeek、Harness 这一周不嵌入。`catalog/apps.json` 里尚未确认的网址和 Bundle ID 保持 null。

## 登录弹窗和 OAuth

`window.open` 以及 `target=_blank` 走 `on_new_window`。新窗口使用 `window_features`，在 macOS 上会套用系统给出的 `WKWebViewConfiguration`，因此和父分区是同一个数据仓库。只允许 `https`、`http` 和 `about`。

OAuth 页面里的 `window.close()` 由文档开头脚本改成设置标题 `__uw_close__`，壳再关掉这个弹窗。弹窗带系统标题栏，脚本没生效时也可以手动关上。主窗口关闭时进程退出，避免登录窗把应用留在后台。

同一 WebView 里的整页跳转（例如账号站再跳回原站）不拦截。用户代理保持 WKWebView 默认的 Safari 形态，不去伪装成 Chrome。

## 怎么验证没有串数据

cookie 和 localStorage 按来源隔离。四个站点的域名本来就不同，只打开它们不能证明仓库是分开的。检查要在四个分区里打开同一个来源。

侧栏底部的「检查隔离」会：

1. 在 `127.0.0.1` 上起一个只返回固定页面的临时 HTTP 服务。`Info.plist` 里的 `NSAllowsLocalNetworking` 允许这个本地 HTTP。
2. 用和正式站点相同的四个持久仓库各开一个 WebView，都加载 `http://127.0.0.1:<端口>/isolation.html`。页面放在窗口外，避免挡住内容区。
3. 每个仓库写入 `localStorage['uw-partition']` 和同名 cookie，值是自己的应用 id。
4. 全部写完再读。任何一个分区读到的不是自己的 id，就判未通过。

逻辑判定在 `partition::isolation_failure`，`cargo test --lib` 会跑共享仓库和 cookie 串数据的失败样本，也会核对四个 UUID 稳定且互不相同。CI 在 macOS 任务里跑这些测试。现场的 WebKit 读写要在 Mac 上点「检查隔离」，Linux 上的浏览器预览没有这些 WebView。

通过之后，侧栏会列出每个分区的标记，以及仓库在磁盘上的位置。正式打开过站点之后，`~/Library/Application Support/com.uniworkspace.ai/partitions/index.json` 里也有同样的 id 和 UUID 对照。macOS 14 及以上还可以对一下 `~/Library/WebKit/com.uniworkspace.ai/WebsiteDataStore/` 下面是不是四个不同的 UUID 目录。

Week 3 继续用这套持久仓库，没有改成内存仓库，也没有把豆包、WorkBuddy、DeepSeek 放进别的存储。分区标识仍然是同一个 UUID v5。验收步骤见 `docs/week3-m1.md`。
