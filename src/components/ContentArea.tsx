import { useLayoutEffect, useRef } from "react";
import { errorText, syncPanes, type PaneSpec } from "../partitions";
import { framesDiffer, type SnapFrame } from "../snap";
import { useShellStore } from "../store";
import { runningInTauri } from "../window";
import { embedTarget, visibleCount, type LayoutMode } from "../workspace";
import { AccessibilityBanner } from "./AccessibilityBanner";

export function ContentArea() {
  const ready = useShellStore((state) => state.ready);

  return (
    <main className="flex min-w-0 flex-1 flex-col bg-shell-canvas">
      <AccessibilityBanner />
      {ready ? <PaneGrid /> : <LoadingState />}
    </main>
  );
}

function LoadingState() {
  return (
    <div className="flex flex-1 items-center justify-center text-sm text-shell-muted">
      正在恢复上次的布局…
    </div>
  );
}

function PaneGrid() {
  const layout = useShellStore((state) => state.layout);
  const slots = useShellStore((state) => state.slots);
  const customApps = useShellStore((state) => state.customApps);
  const focusedPane = useShellStore((state) => state.focusedPane);
  const reopenNonce = useShellStore((state) => state.reopenNonce);
  const syncError = useShellStore((state) => state.syncError);
  const setSyncError = useShellStore((state) => state.setSyncError);
  const hosts = useRef<(HTMLDivElement | null)[]>([]);
  const lastSnapFrame = useRef<SnapFrame | null>(null);
  const snap = useShellStore((state) => state.snap);
  const snapRequest = useShellStore((state) => state.snapRequest);
  const snapBusy = useShellStore((state) => state.snapBusy);
  const count = visibleCount(layout);

  useLayoutEffect(() => {
    if (!runningInTauri()) {
      return;
    }
    let frameId = 0;
    const publish = () => {
      const state = useShellStore.getState();
      const visible = visibleCount(state.layout);
      const panes: PaneSpec[] = [];
      for (let index = 0; index < visible; index += 1) {
        if (state.snap?.paneIndex === index) {
          continue;
        }
        const appId = state.slots[index];
        const host = hosts.current[index];
        if (!appId || !host) {
          continue;
        }
        const target = embedTarget(appId, state.customApps);
        if (!target) {
          continue;
        }
        const rect = host.getBoundingClientRect();
        if (rect.width < 2 || rect.height < 2) {
          continue;
        }
        panes.push({
          appId,
          url: target.url,
          x: rect.x,
          y: rect.y,
          width: rect.width,
          height: rect.height,
          focus: index === state.focusedPane,
        });
      }
      void syncPanes(panes)
        .then(() => {
          setSyncError(null);
        })
        .catch((reason: unknown) => {
          setSyncError(errorText(reason));
        });
      if (state.snap && state.snap.paneIndex >= visible && !state.snapBusy) {
        void state.detachSnap();
        return;
      }
      if (state.snapRequest !== null && !state.snapBusy) {
        const rect = measureHost(hosts.current[state.snapRequest]);
        if (rect) {
          void state.attachSnap(state.snapRequest, rect);
        }
        return;
      }
      if (!state.snap || state.snapBusy) {
        lastSnapFrame.current = null;
        return;
      }
      const rect = measureHost(hosts.current[state.snap.paneIndex]);
      if (!rect) {
        return;
      }
      if (!lastSnapFrame.current || framesDiffer(lastSnapFrame.current, rect)) {
        lastSnapFrame.current = rect;
        void state.updateSnapFrame(rect);
      }
    };
    const schedule = () => {
      cancelAnimationFrame(frameId);
      frameId = requestAnimationFrame(publish);
    };
    schedule();
    const observer = new ResizeObserver(() => {
      schedule();
    });
    for (let index = 0; index < count; index += 1) {
      const host = hosts.current[index];
      if (host) {
        observer.observe(host);
      }
    }
    window.addEventListener("resize", schedule);
    return () => {
      cancelAnimationFrame(frameId);
      observer.disconnect();
      window.removeEventListener("resize", schedule);
    };
  }, [count, customApps, focusedPane, layout, reopenNonce, setSyncError, slots, snap, snapBusy, snapRequest]);

  const gridClass =
    layout === "single"
      ? "grid-cols-1 grid-rows-1"
      : layout === "split"
        ? "grid-cols-2 grid-rows-1"
        : "grid-cols-2 grid-rows-2";

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex h-10 shrink-0 items-center gap-2 border-b border-shell-line px-3">
        <LayoutButton mode="single" label="单窗" />
        <LayoutButton mode="split" label="1:1" />
        <LayoutButton mode="grid" label="田字格" />
        <ClearButton />
      </div>
      {syncError ? (
        <p className="shrink-0 px-3 py-2 text-xs leading-5 text-[#8a3d32]">{syncError}</p>
      ) : null}
      <div className={`grid min-h-0 flex-1 gap-2 p-2 ${gridClass}`}>
        {Array.from({ length: count }, (_, index) => (
          <Pane
            key={`${layout}-${index}`}
            index={index}
            hostRef={(element) => {
              hosts.current[index] = element;
            }}
          />
        ))}
      </div>
    </div>
  );
}

function LayoutButton({ mode, label }: { mode: LayoutMode; label: string }) {
  const layout = useShellStore((state) => state.layout);
  const setLayout = useShellStore((state) => state.setLayout);
  const active = layout === mode;

  return (
    <button
      type="button"
      aria-pressed={active}
      className={`rounded-md px-2.5 py-1 text-xs ${
        active
          ? "bg-white text-shell-ink shadow-sm"
          : "text-shell-muted hover:bg-black/5 hover:text-shell-ink"
      }`}
      onClick={() => {
        void setLayout(mode);
      }}
    >
      {label}
    </button>
  );
}

function ClearButton() {
  const clearAllPanes = useShellStore((state) => state.clearAllPanes);
  return (
    <button
      type="button"
      className="ml-auto rounded-md px-2 py-1 text-xs text-shell-muted hover:bg-black/5 hover:text-shell-ink"
      onClick={() => {
        void clearAllPanes();
      }}
    >
      清空分屏
    </button>
  );
}

function measureHost(host: HTMLDivElement | null | undefined): SnapFrame | null {
  if (!host) {
    return null;
  }
  const rect = host.getBoundingClientRect();
  if (rect.width < 2 || rect.height < 2) {
    return null;
  }
  return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
}

function Pane({
  index,
  hostRef,
}: {
  index: number;
  hostRef: (element: HTMLDivElement | null) => void;
}) {
  const appId = useShellStore((state) => state.slots[index] ?? null);
  const customApps = useShellStore((state) => state.customApps);
  const focused = useShellStore((state) => state.focusedPane === index);
  const note = useShellStore((state) => (appId ? state.paneNotes[appId] : undefined));
  const snap = useShellStore((state) => (state.snap?.paneIndex === index ? state.snap : null));
  const focusPane = useShellStore((state) => state.focusPane);
  const clearPane = useShellStore((state) => state.clearPane);
  const detachSnap = useShellStore((state) => state.detachSnap);
  const target = appId ? embedTarget(appId, customApps) : null;
  const embedded = snap?.mode === "embedded";

  return (
    <section
      className={`flex min-h-0 min-w-0 flex-col overflow-hidden rounded-lg border ${
        embedded ? "bg-shell-canvas" : "bg-white"
      } ${focused ? "border-shell-accent" : "border-shell-line"}`}
      onMouseDown={() => {
        void focusPane(index);
      }}
    >
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-shell-line bg-white px-2">
        <span className="truncate text-xs font-medium">
          {snap ? "Claude Desktop" : target?.name ?? "空格"}
        </span>
        {note && !snap ? (
          <span className="truncate text-[11px] text-shell-accent">{note}</span>
        ) : null}
        {snap ? (
          <button
            type="button"
            className="ml-auto shrink-0 rounded px-1.5 py-0.5 text-[11px] text-shell-muted hover:bg-black/5 hover:text-shell-ink"
            onMouseDown={(event) => event.stopPropagation()}
            onClick={() => {
              void detachSnap();
            }}
          >
            拆卸
          </button>
        ) : appId ? (
          <button
            type="button"
            className="ml-auto shrink-0 rounded px-1.5 py-0.5 text-[11px] text-shell-muted hover:bg-black/5 hover:text-shell-ink"
            onMouseDown={(event) => event.stopPropagation()}
            onClick={() => {
              void clearPane(index);
            }}
          >
            移出
          </button>
        ) : null}
      </div>
      <div
        ref={hostRef}
        className={`flex min-h-0 flex-1 items-center justify-center px-4 text-center text-sm leading-6 text-shell-muted ${
          embedded ? "bg-transparent" : ""
        }`}
      >
        {embedded ? (
          <div className="pointer-events-none flex h-full w-full items-start justify-center rounded-sm border border-dashed border-shell-line pt-3 text-xs">
            透明占位。Claude Desktop 会对齐到这块区域。
          </div>
        ) : snap ? (
          snap.message
        ) : target ? (
          runningInTauri() ? (
            "正在打开这一格的分区。"
          ) : (
            `${target.url}。浏览器预览不会加载这个页面。`
          )
        ) : (
          "从侧栏把已确认的网页放进这一格。"
        )}
      </div>
    </section>
  );
}
