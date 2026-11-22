import { useState } from "react";
import { appsByKind, catalog } from "../catalog";
import { comfyOpenUrl, type ListedWindow, type ServiceStatus } from "../desktop";
import { useShellStore } from "../store";
import { runningInTauri } from "../window";

const confirmedIds = new Set(
  catalog.apps
    .filter((app) => app.kind === "native" && app.bundleIdStatus === "confirmed")
    .map((app) => app.id),
);

export function DesktopPanels() {
  const nativeApps = appsByKind("native");
  const detectedApps = useShellStore((state) => state.detectedApps);
  const scanned = useShellStore((state) => state.scanned);
  const scanError = useShellStore((state) => state.scanError);
  const windows = useShellStore((state) => state.windows);
  const windowNote = useShellStore((state) => state.windowNote);
  const designated = useShellStore((state) => state.designated);
  const services = useShellStore((state) => state.services);
  const inspectedId = useShellStore((state) => state.inspectedId);
  const inspect = useShellStore((state) => state.inspect);
  const designate = useShellStore((state) => state.designate);
  const undesignate = useShellStore((state) => state.undesignate);
  const reopenA11y = useShellStore((state) => state.reopenA11y);
  const addCustomUrl = useShellStore((state) => state.addCustomUrl);
  const askSnap = useShellStore((state) => state.askSnap);
  const snap = useShellStore((state) => state.snap);
  const snapError = useShellStore((state) => state.snapError);
  const snapNote = useShellStore((state) => state.snapNote);
  const a11yChoice = useShellStore((state) => state.a11yChoice);
  const a11yTrusted = useShellStore((state) => state.a11yTrusted);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerNote, setPickerNote] = useState<string | null>(null);
  const [opening, setOpening] = useState(false);
  const inApp = runningInTauri();
  const running = detectedApps.filter((app) => confirmedIds.has(app.appId));

  return (
    <>
      <section>
        <div className="flex items-center justify-between px-2 pb-1">
          <h2 className="text-[11px] font-medium text-shell-muted">桌面</h2>
          <button
            type="button"
            className="text-[11px] text-shell-muted hover:text-shell-ink"
            onClick={reopenA11y}
          >
            辅助功能
          </button>
        </div>
        <div className="px-2 pb-2" aria-live="polite">
          <p className="text-[11px] font-medium text-shell-muted">运行中</p>
          {!inApp ? (
            <p className="mt-1 text-xs leading-5 text-shell-muted">桌面检测要在 macOS 应用里看。</p>
          ) : !scanned ? (
            <p className="mt-1 text-xs leading-5 text-shell-muted">正在检测已确认的桌面应用。</p>
          ) : running.length === 0 ? (
            <p className="mt-1 text-xs leading-5 text-shell-muted">
              还没有检测到已确认的桌面应用。
            </p>
          ) : (
            <ul className="mt-1 space-y-0.5">
              {running.map((app) => (
                <li
                  key={app.appId}
                  title={`${app.bundleId} · ${app.processName} · ${app.pid}`}
                  className="flex items-center rounded-md px-2 py-1.5 text-sm hover:bg-black/5"
                >
                  <button
                    type="button"
                    className="min-w-0 flex-1 truncate text-left"
                    onClick={() => {
                      inspect(app.appId);
                    }}
                  >
                    {app.name}
                  </button>
                  <button
                    type="button"
                    className="ml-2 shrink-0 text-[10px] text-shell-accent"
                    onClick={() => {
                      if (snap?.appId !== app.appId) {
                        askSnap(app.appId);
                      }
                    }}
                  >
                    {snap?.appId === app.appId ? "跟随中" : "吸附"}
                  </button>
                  <span className="ml-2 shrink-0 text-[10px] text-shell-accent">运行中</span>
                </li>
              ))}
            </ul>
          )}
        </div>
        <ul className="space-y-0.5">
          {nativeApps.map((app) => {
            const live = running.some((item) => item.appId === app.id);
            let badge = "待确认";
            if (app.bundleIdStatus === "confirmed") {
              if (!inApp) {
                badge = "";
              } else if (!scanned) {
                badge = "检测中";
              } else {
                badge = live ? "运行中" : "未运行";
              }
            }
            const selected = inspectedId === app.id;
            return (
              <li key={app.id}>
                <button
                  type="button"
                  aria-current={selected ? "true" : undefined}
                  title={app.bundleId ?? undefined}
                  className={`flex w-full items-center justify-between rounded-md px-2 py-1.5 text-left text-sm ${
                    selected ? "bg-white text-shell-ink shadow-sm" : "text-shell-ink hover:bg-black/5"
                  }`}
                  onClick={() => {
                    inspect(app.id);
                  }}
                >
                  <span className="truncate">{app.name}</span>
                  {badge ? (
                    <span
                      className={`ml-2 shrink-0 text-[10px] ${
                        badge === "运行中" ? "text-shell-accent" : "text-shell-muted"
                      }`}
                    >
                      {badge}
                    </span>
                  ) : null}
                </button>
              </li>
            );
          })}
        </ul>
        {snap ? (
          <p className="px-2 pt-2 text-[11px] leading-5 text-shell-muted">{snap.message}</p>
        ) : null}
        {a11yChoice === "webOnly" || (inApp && scanned && !a11yTrusted) ? (
          <p className="px-2 pt-2 text-[11px] leading-5 text-shell-muted">
            网页分屏可以继续使用。桌面窗口不会被移动。
          </p>
        ) : null}
        {snapNote ? <p className="px-2 pt-2 text-[11px] leading-5 text-shell-muted">{snapNote}</p> : null}
        {snapError ? <p className="px-2 pt-2 text-[11px] leading-5 text-[#8a3d32]">{snapError}</p> : null}
        {scanError ? <p className="px-2 pt-2 text-[11px] leading-5 text-[#8a3d32]">{scanError}</p> : null}
      </section>
      <section className="px-2">
        <button
          type="button"
          aria-expanded={pickerOpen}
          className="w-full rounded-md border border-shell-line bg-white px-2 py-1.5 text-left text-xs text-shell-ink hover:bg-black/5"
          onClick={() => {
            if (!inApp) {
              setPickerNote("指定窗口要在 macOS 应用里使用。");
              setPickerOpen(false);
              return;
            }
            setPickerNote(null);
            setPickerOpen((value) => !value);
          }}
        >
          指定窗口
        </button>
        <p className="mt-1 text-[11px] leading-5 text-shell-muted">
          选一个可见窗口放进侧栏。只列出，不移动它。
        </p>
        {pickerNote ? <p className="mt-1 text-[11px] leading-5 text-[#8a3d32]">{pickerNote}</p> : null}
        {pickerOpen ? (
          <div className="mt-1 max-h-48 overflow-y-auto rounded-md border border-shell-line bg-white">
            {windowNote ? <p className="px-2 py-1 text-[11px] leading-5 text-shell-muted">{windowNote}</p> : null}
            {windows.length === 0 ? (
              <p className="px-2 py-2 text-[11px] leading-5 text-shell-muted">没有可指定的可见窗口。</p>
            ) : (
              <ul>
                {windows.map((item) => {
                  const added = designated.some((entry) => entry.windowId === item.windowId);
                  return (
                    <li key={item.windowId} className="flex items-center gap-1 border-t border-shell-line px-2 py-1 first:border-t-0">
                      <span className="min-w-0 flex-1 truncate text-xs" title={item.label}>
                        {item.label}
                      </span>
                      <button
                        type="button"
                        disabled={added}
                        className="shrink-0 text-[10px] text-shell-accent disabled:text-shell-muted"
                        onClick={() => {
                          designate(item);
                        }}
                      >
                        {added ? "已加入" : "加入侧栏"}
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        ) : null}
        {designated.length > 0 ? (
          <ul className="mt-2 max-h-32 space-y-0.5 overflow-y-auto">
            {designated.map((item) => {
              const live = windows.find((entry) => entry.windowId === item.windowId);
              return (
                <DesignatedRow
                  key={item.windowId}
                  item={item}
                  label={live?.label ?? item.label}
                  open={Boolean(live)}
                  onRemove={() => {
                    undesignate(item.windowId);
                  }}
                />
              );
            })}
          </ul>
        ) : null}
      </section>
      <section className="px-2">
        <h2 className="pb-1 text-[11px] font-medium text-shell-muted">本机端口</h2>
        {!inApp ? (
          <p className="text-xs leading-5 text-shell-muted">端口状态要在 macOS 应用里看。</p>
        ) : !scanned ? (
          <p className="text-xs leading-5 text-shell-muted">正在检测 127.0.0.1 的 8188、11434、1234。</p>
        ) : (
          <ul className="space-y-1">
            {services.map((service) => (
              <ServiceRow
                key={service.id}
                service={service}
                opening={opening}
                onOpen={(url) => {
                  setOpening(true);
                  void addCustomUrl(url, "ComfyUI").finally(() => {
                    setOpening(false);
                  });
                }}
              />
            ))}
          </ul>
        )}
        <p className="mt-1 text-[11px] leading-5 text-shell-muted">
          只显示是否在监听。打开 ComfyUI 只会加载 http://127.0.0.1:8188/。
        </p>
      </section>
    </>
  );
}

function DesignatedRow({
  item,
  label,
  open,
  onRemove,
}: {
  item: ListedWindow;
  label: string;
  open: boolean;
  onRemove: () => void;
}) {
  return (
    <li className="flex items-center gap-1 rounded-md px-2 py-1 hover:bg-black/5">
      <span className="min-w-0 flex-1 truncate text-xs" title={item.bundleId ?? label}>
        {label}
      </span>
      <span className="shrink-0 text-[10px] text-shell-muted">{open ? "可见" : "已关闭"}</span>
      <button type="button" className="shrink-0 text-[10px] text-shell-muted hover:text-shell-ink" onClick={onRemove}>
        移除
      </button>
    </li>
  );
}

function ServiceRow({
  service,
  opening,
  onOpen,
}: {
  service: ServiceStatus;
  opening: boolean;
  onOpen: (url: string) => void;
}) {
  const url = comfyOpenUrl(service);
  return (
    <li className="flex items-center gap-1 text-xs">
      <span className="min-w-0 flex-1 truncate">
        {service.name}
        <span className="text-shell-muted"> · {service.address}</span>
      </span>
      <span className={service.running ? "shrink-0 text-[10px] text-shell-accent" : "shrink-0 text-[10px] text-shell-muted"}>
        {service.running ? "运行中" : "未运行"}
      </span>
      {url ? (
        <button
          type="button"
          disabled={opening}
          title="在网页视图中打开 http://127.0.0.1:8188/"
          className="shrink-0 text-[10px] text-shell-accent disabled:opacity-50"
          onClick={() => {
            onOpen(url);
          }}
        >
          打开
        </button>
      ) : null}
    </li>
  );
}
