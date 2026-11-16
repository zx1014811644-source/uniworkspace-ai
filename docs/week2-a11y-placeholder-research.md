# Week 2：辅助功能与透明占位调研

这一周只记录 macOS 上以后做几何吸附要用的接口、权限提示和限制。不实现吸附，不跟着别的窗口走，也不做边条。`window_mac` 里只有注释，不会调用辅助功能 API，因此不会弹出授权框。

## 建议

Week 4 以后的吸附目标，是主窗口内容区或分屏格里的一块矩形，不是把别的进程嵌进我们的视图层级。macOS 没有 Windows 那种 `SetParent`。能做的是：读出对方窗口的位置，再在用户已经授权辅助功能的前提下，把它挪到占位矩形上。

占位矩形用普通 `NSView` 的 `frame` 即可，坐标系和 Week 2 分区 WebView 已经在用的内容区 bounds 相同。壳窗口保持不透明。整窗透明会和自定义标题栏、WKWebView 叠在一起，这一周不改 `transparent`。

跟随只在有窗口被吸附时打开。用 `CVDisplayLink`（macOS 13 上仍可用；更新的接口是挂在视图上的 `CADisplayLink`）在刷新回调里算矩形，再派回主队列改对方窗口的 frame。没有吸附对象时停掉 Display Link，不要常驻。

实现放在现有的 `window_mac`，用 objc2 调公开 API。只有走到私有 SkyLight / `SLS` 调用、而且 Rust 绑定实在难写时，才单独加一个 Swift helper。以目前的公开接口，不需要第二条语言工具链。

## 辅助功能

读窗口几何可以先用 `CGWindowListCopyWindowInfo`。它不弹辅助功能授权，能拿到屏幕上的窗口 bounds，但不能移动别人的窗口，也不能稳定地只锁定「前台那个可移动的主窗口」。列表里还有状态栏、菜单和各种子窗口，后面要自己过滤。

移动和改变大小要用辅助功能：

- `AXIsProcessTrusted()` 只查询，不提示。
- `AXIsProcessTrustedWithOptions`，选项字典里 `kAXTrustedCheckOptionPrompt` 设为 true 时，系统会提示并把用户带去「系统设置 → 隐私与安全性 → 辅助功能」。提示文案不能自定义，显示的是包名。
- 授权之后用 `AXUIElementCreateApplication(pid)` 拿到应用，再取 `kAXWindowsAttribute`。
- 位置和大小是 `kAXPositionAttribute`、`kAXSizeAttribute`。读用 `AXUIElementCopyAttributeValue`，写用 `AXUIElementSetAttributeValue`。
- 变化通知用 `AXObserver`，关注 `kAXMovedNotification`、`kAXResizedNotification`、`kAXWindowCreatedNotification`。
- 这些调用放在主线程。

坐标：辅助功能的原点在屏幕左上，AppKit 屏幕坐标的原点在左下。写回 position 之前要换算，多显示器时用窗口所在那块屏的高度。

## 权限和限制

第一次带 prompt 选项调用时才会出现系统对话框。用户关掉对话框之后，仍可能要在设置里手动打开开关。未签名的 debug 构建每次产物路径或 ad-hoc 签名变了，系统都可能把它当成新应用，授权不会跟着走。Week 4 的引导界面要预期「已经点过允许，但列表里是关的」。

辅助功能权限是 TCC，不是 Info.plist 里的用途字符串。加上 App Sandbox 之后，跨应用移动窗口这条路基本走不通。当前包未签名、也没有沙箱，保持这样，直到以后单独做公证。

有些 Electron 应用会忽略辅助功能发起的移动。失败要留在原处并告诉用户，不能循环重试。`AXUIElementSetAttributeValue` 返回错误时就停。

macOS 13 和 macOS 14 的这套 AX API 没有换成别的公开接口。没有一种办法可以不经辅助功能授权就去改别人的窗口 frame。

## 透明占位

吸附用的占位不是「把窗口挖空、透出后面的程序」。那需要整窗透明、WebView 不透明区域抠洞，标题栏拖拽和分区 WebView 的命中测试都会变难。

Week 4 的占位视图：

- 放在主窗口 content view 里，frame 等于当前分屏格。
- `setWantsLayer:YES`，图层背景用 `clearColor`，这样它不盖住壳的内容，也不需要窗口本身透明。
- 它只提供几何，不负责显示对方的像素。对方窗口是操作系统里的另一个 `NSWindow`，由 AX 挪到这个 frame 的屏幕坐标上。

Week 2 的分区 WebView 已经按内容区的 `getBoundingClientRect()` 在主窗口里定位。以后的占位矩形沿用同一套 bounds，避免壳和原生视图各算各的。

## 这一周明确不做

不请求辅助功能权限，不创建 `AXObserver`，不创建 Display Link，不插入占位 `NSView`，不跟随窗口，不做边条。权限引导界面留到 Week 4–6。
