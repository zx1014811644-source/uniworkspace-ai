import { useLayoutEffect, useRef } from "react";
import { errorText, syncPanes, type PaneSpec } from "../partitions";
import { useShellStore } from "../store";
import { runningInTauri } from "../window";
import { embedTarget, visibleCount, type LayoutMode } from "../workspace";

export function ContentArea() {
  const ready = useShellStore((state) => state.ready);

  return (
    <main className="flex min-w-0 flex-1 flex-col bg-shell-canvas">
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
  const syncError = useShellStore((state) => state.syncError);
  const setSyncError = useShellStore((state) => state.setSyncError);
  const hosts = useRef<(HTMLDivElement | null)[]>([]);
  const count = visibleCount(layout);

  useLayoutEffect(() => {
    if (!runningInTauri()) {
      return;
    }
    let frame = 0;
    const publish = () => {
      const state = useShellStore.getState();
      const visible = visibleCount(state.layout);
      const panes: PaneSpec[] = [];
      for (let index = 0; index < visible; index += 1) {
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
    };
    const schedule = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(publish);
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
      cancelAnimationFrame(frame);
      observer.disconnect();
      window.removeEventListener("resize", schedule);
    };
  }, [count, customApps, focusedPane, layout, setSyncError, slots]);

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
  const focusPane = useShellStore((state) => state.focusPane);
  const clearPane = useShellStore((state) => state.clearPane);
  const target = appId ? embedTarget(appId, customApps) : null;

  return (
    <section
      className={`flex min-h-0 min-w-0 flex-col overflow-hidden rounded-lg border bg-white ${
        focused ? "border-shell-accent" : "border-shell-line"
      }`}
      onMouseDown={() => {
        void focusPane(index);
      }}
    >
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-shell-line px-2">
        <span className="truncate text-xs font-medium">{target?.name ?? "空格"}</span>
        {note ? <span className="truncate text-[11px] text-shell-accent">{note}</span> : null}
        {appId ? (
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
        className="flex min-h-0 flex-1 items-center justify-center px-4 text-center text-sm leading-6 text-shell-muted"
      >
        {target ? (
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
