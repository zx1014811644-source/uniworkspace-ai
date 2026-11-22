# Week 7：热键、权限降级与稳定性（M3）

这些步骤要在 macOS 13 或更高版本上执行。云端和 Linux 不能跑 `pnpm tauri dev` 的真实全局热键，也不能切换 macOS 桌面空间，所以下面每一项都要在 Mac 上点验。云端 CI 只证明未签名 debug 包能编过，并且目录检查和单元测试通过。

```bash
pnpm install
pnpm tauri dev
```

退出时点窗口左上角的红色关闭按钮。关闭时会先把已经吸附的窗口还原，再结束进程。

快捷键写在：

`~/Library/Application Support/com.uniworkspace.ai/shortcut.json`

不要手改这个文件。改热键用内容区里的「热键」按钮。

## 全局热键

默认是 Option+Space。它不需要辅助功能权限。

1. 打开应用，看内容区顶部的「热键 Option+Space」。
2. 把主窗口留在当前空间，并点一下它，让它成为焦点。再按 Option+Space。主窗口应隐藏。
3. 再按一次。主窗口回到当前空间并成为焦点。
4. 点到别的应用上，让 UniWorkspace 不再是焦点，但窗口还在当前空间。按 Option+Space。主窗口应回到前面，而不是先被隐藏。再按一次才隐藏。
5. 用触控板或 Mission Control 切到另一个 Space，窗口留在原来的空间。在新空间按 Option+Space。主窗口应出现在这个空间里，而不是只在原来的空间里闪一下。
6. 在一个全屏应用的空间里再按一次。主窗口应能出现在这个空间上。再按一次（它已是焦点时）应隐藏。
7. 热键不会移动其他应用的窗口。指定窗口的位置不变。

录制新热键时，当前热键会先暂时关掉，避免 Option+Space 在录入过程中把窗口藏起来。按 Esc 会取消，并恢复原来的热键。

## 不要和系统快捷键卡死

下面这些组合和 macOS 自己的快捷键撞车。点「热键」，按下它们，应出现说明，热键不会被注册，也不会反复重试。应用不退出，网页分屏继续能用。

| 按下 | 系统用途 |
| --- | --- |
| Command+Space | 聚焦搜索 |
| Command+Option+Space | 访达搜索 |
| Control+Space | 上一个输入法 |
| Control+Option+Space | 输入法菜单里的下一个 |
| Command+Control+Space | 表情与符号 |

可以改成例如 Option+Shift+Space，或 Control+Shift+K。改完之后，新组合能隐藏和唤出主窗口，旧的 Option+Space 不再占用。只按 Shift 加一个字母会被拒绝，避免和打字抢键。

如果系统或其他应用已经占用了你选的组合，`RegisterEventHotKey` 失败时只报告一次，不会在回调里等待主线程。等待会和系统快捷键互相卡住。内容区应写明没有注册、没有重试。原来还能用的热键应留着。

## 辅助功能被拒绝

1. 点「仅使用网页」，或者在系统对话框里不允许。系统里已经开过辅助功能时，也要再点一次「仅使用网页」做这一步。
2. 点任意已确认桌面应用的「吸附」。
3. 侧栏说明需要辅助功能，并写明这个窗口没有被移动。它停在原来的位置，边框也不变。
4. 侧栏点 ChatGPT 或 Claude 网页。分屏、登录和自定义链接仍可使用。应用不退出。
5. 热键仍能隐藏和唤出主窗口。唤出时不要把别人的窗口拽过来。

选「仅使用网页」时，如果当时已经有一个窗口吸附着，会先按 Week 6 的拆卸把它还原，然后才停止移动。还原是这一次收尾，之后的跟随不再写它的位置。

启动时不会自己弹出系统授权。

## 非当前格暂停

1. 切到田字格，把四个已确认网页放进四格，例如 ChatGPT、Claude、Gemini、DeepSeek。
2. 点其中一格。这一格没有「已暂停」。另外三格的标题旁应出现「已暂停」。
3. 在当前格里打字、滚动。另外三格不应继续播放视频或不停地自己刷新。把其中一格点成当前格后，它应恢复，刚才那一格变成「已暂停」。
4. 登录态还在。暂停不应该把这一格退出登录，也不应该让其他格重新加载。
5. 用热键隐藏主窗口。四格都应暂停。再唤出主窗口。只有当前格恢复，其他格仍暂停。
6. 某一格的网页崩溃时，仍然只重建那一格。当前格重建后应能继续输入。

暂停用的是 macOS 12 起就有的 `WKWebView` `_suspendPage:` / `_resumePage:`，外加暂停该页媒体。当前格不会被暂停。主窗口自己的界面不会被暂停。

## 吸附仍然是一个窗口

Week 6 的单窗口吸附不变。再点另一个已确认应用的「吸附」时，先还原上一个，再吸附新的。不要为了「最多两个」同时吸住两个原生窗口。

建议上限是同时两个原生窗口。这一周只把这个上限写在 `catalog/compatibility.json` 的 `nativeSnapCeiling` 里，`nativeSnapLimit` 仍是 1。多窗口吸附留到以后，不在这次改跟随、拆卸或还原。

吸附、跟随、全屏、Space 边缘、对方先退出和拆卸，仍按 `docs/week6-m2.md` 点验一遍。拖动时允许轻微缝隙。

## 指定窗口

1. 点「指定窗口」，把任意可见窗口加入侧栏。
2. 那个窗口的位置、大小和层级都不变。
3. 这一行没有「吸附」。热键、暂停网页、仅使用网页，都不会去移动它。

## 兼容清单

只点验目录里已经确认的入口。下表的网址和 Bundle ID 都来自 `catalog/apps.json`，没有新编。

待确认的三项网页保持「不打开」，网址保持空：Accio Work、Cursor、Harness。WorkBuddy Desktop、Harness Desktop、Accio Work Desktop 的 Bundle ID 保持空。`com.todesktop.230313mzl4w4u92` 只属于 Cursor Desktop。

### 已确认网页

每一项都要能放进分屏、保持自己的登录分区，并且在不是当前格时暂停。

| 应用 | 目录 id | 网址 |
| --- | --- | --- |
| ChatGPT | `chatgpt` | `https://chatgpt.com/` |
| Claude | `claude` | `https://claude.ai/` |
| Gemini | `gemini` | `https://gemini.google.com/` |
| Kimi | `kimi` | `https://www.kimi.com/` |
| 豆包 | `doubao` | `https://www.doubao.com/chat/` |
| WorkBuddy | `workbuddy` | `https://www.workbuddy.cn/app` |
| DeepSeek | `deepseek` | `https://chat.deepseek.com/` |

点验时看这几件事：页面能打开，登录框或已登录的会话在；刷新或崩溃只影响这一格；切走之后标题出现「已暂停」，切回来还能输入。发现页面白屏、登录被清掉，或暂停后当前格也不能输入，记成 P0，先修再继续后面的项。

### 已确认桌面应用

每一项都要能在已授权时吸附、点击、跟随和拆卸。拒绝辅助功能或「仅使用网页」时，点「吸附」不得移动它。

| 应用 | 目录 id | Bundle ID |
| --- | --- | --- |
| ChatGPT Desktop | `chatgpt-desktop` | `com.openai.codex` |
| Claude Desktop | `claude-desktop` | `com.anthropic.claudefordesktop` |
| LM Studio | `lm-studio` | `ai.elementlabs.lmstudio` |
| Gemini Desktop | `gemini-desktop` | `com.google.GeminiMacOS` |
| Kimi Desktop | `kimi-desktop` | `com.moonshot.kimichat` |
| 豆包 Desktop | `doubao-desktop` | `com.bot.pc.doubao` |
| Cursor Desktop | `cursor-desktop` | `com.todesktop.230313mzl4w4u92` |

没有安装的应用跳过，不要为了点验去猜别的 Bundle ID。

### 不打开

| 应用 | 目录 id | 保持为空的字段 |
| --- | --- | --- |
| Accio Work | `accio-work` | `webUrl` |
| Cursor | `cursor` | `webUrl` |
| Harness | `harness` | `webUrl` |
| WorkBuddy Desktop | `workbuddy-desktop` | `bundleId` |
| Harness Desktop | `harness-desktop` | `bundleId` |
| Accio Work Desktop | `accio-work-desktop` | `bundleId` |

点这些网页项，侧栏仍是「不打开」，内容区不出现新的 WebView。

## 本周处理过的问题

选择「仅使用网页」时，如果系统辅助功能其实已经打开，Week 6 的吸附仍会移动窗口。现在这条路在移动之前就返回，侧栏写明没有移动。跟随循环在「仅使用网页」期间也不再写别人的窗口位置。

非当前格以前会和当前格一起持续跑脚本。现在会暂停，避免四个已确认网页同时占着 CPU。

`shortcuts` 不再是占位。全局热键会真正注册。

`catalog/compatibility.json` 的 `openP0` 目前是空的。点验中新发现的 P0 先修，再把还没修完的项记在这里，不要把未确认的网址或 Bundle ID 写进去。

## 不要回归的部分

已确认网页的分区、登录态、单窗 / 1:1 / 田字格，以及只重建崩溃的那一格，都还在。ComfyUI 仍只打开 `http://127.0.0.1:8188/`。Week 6 的单窗口吸附、拆卸和还原还在。指定窗口仍然只列出。

## 这一周明确不做

不把原生吸附扩成两个窗口。不做安装包，不做公证，不做 Windows 或 Linux 构建。不做窗口镜像。
