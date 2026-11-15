import { useLayoutEffect, useRef, useState } from "react";
import { catalog } from "../catalog";
import {
  errorText,
  hidePartitions,
  isWeek2Web,
  setPartitionBounds,
  showPartition,
} from "../partitions";
import { useShellStore } from "../store";
import type { CatalogApp } from "../types";
import { runningInTauri } from "../window";

export function ContentArea() {
  const selectedId = useShellStore((state) => state.selectedId);
  const selected = catalog.apps.find((app) => app.id === selectedId) ?? null;

  return (
    <main className="relative min-w-0 flex-1 overflow-hidden bg-shell-canvas">
      {selected && isWeek2Web(selected) ? (
        <PartitionHost app={selected} />
      ) : selected ? (
        <SelectedApp />
      ) : (
        <EmptyState />
      )}
    </main>
  );
}

function EmptyState() {
  return (
    <div className="flex h-full items-center justify-center px-10">
      <div className="max-w-md text-center">
        <p className="text-xs font-semibold uppercase tracking-[0.18em] text-shell-accent">
          内容区
        </p>
        <h1 className="mt-3 text-2xl font-semibold tracking-tight">选择一个应用</h1>
        <p className="mt-3 text-sm leading-6 text-shell-muted">
          ChatGPT、Claude、Gemini 和 Kimi 会在各自的分区里打开。其余应用这一周仍然只标出位置。
        </p>
      </div>
    </div>
  );
}

function SelectedApp() {
  const selectedId = useShellStore((state) => state.selectedId);
  const app = catalog.apps.find((entry) => entry.id === selectedId);
  if (!app) {
    return <EmptyState />;
  }

  const status =
    app.kind === "web"
      ? app.webStatus === "official"
        ? "已记录官方网址，这一周还不会在这里打开"
        : "官方网址仍待确认"
      : app.bundleIdStatus === "confirmed"
        ? "已记录 Bundle ID，尚未附着窗口"
        : "Bundle ID 仍待确认";

  return (
    <div className="flex h-full items-center justify-center px-10">
      <div className="w-full max-w-lg rounded-2xl border border-shell-line bg-white px-8 py-7 shadow-sm">
        <p className="text-xs font-semibold uppercase tracking-[0.18em] text-shell-accent">
          {app.kind === "web" ? "网页" : "桌面"}
        </p>
        <h1 className="mt-2 text-2xl font-semibold tracking-tight">{app.name}</h1>
        <p className="mt-3 text-sm leading-6 text-shell-muted">{status}</p>
      </div>
    </div>
  );
}

function PartitionHost({ app }: { app: CatalogApp & { webUrl: string } }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);

  useLayoutEffect(() => {
    if (!runningInTauri()) {
      return;
    }
    const host = hostRef.current;
    if (!host) {
      return;
    }
    let cancelled = false;
    let shown = false;
    let lastKey = "";

    const publish = () => {
      if (cancelled) {
        return;
      }
      const rect = host.getBoundingClientRect();
      if (rect.width < 2 || rect.height < 2) {
        return;
      }
      const key = [
        rect.x.toFixed(1),
        rect.y.toFixed(1),
        rect.width.toFixed(1),
        rect.height.toFixed(1),
      ].join(":");
      if (key === lastKey) {
        return;
      }
      lastKey = key;
      const bounds = {
        appId: app.id,
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
      };
      if (!shown) {
        shown = true;
        void showPartition({ ...bounds, url: app.webUrl }).catch((reason: unknown) => {
          if (!cancelled) {
            setError(errorText(reason));
          }
        });
        return;
      }
      void setPartitionBounds(bounds);
    };

    publish();
    const observer = new ResizeObserver(() => publish());
    observer.observe(host);
    window.addEventListener("resize", publish);
    return () => {
      cancelled = true;
      observer.disconnect();
      window.removeEventListener("resize", publish);
      void hidePartitions();
    };
  }, [app.id, app.webUrl]);

  if (!runningInTauri()) {
    return (
      <div className="flex h-full items-center justify-center px-10">
        <div className="w-full max-w-lg rounded-2xl border border-shell-line bg-white px-8 py-7 shadow-sm">
          <p className="text-xs font-semibold uppercase tracking-[0.18em] text-shell-accent">
            网页
          </p>
          <h1 className="mt-2 text-2xl font-semibold tracking-tight">{app.name}</h1>
          <p className="mt-3 text-sm leading-6 text-shell-muted">
            浏览器预览里没有分区 WebView。在 Mac 上运行 <code>pnpm tauri dev</code> 后，这里会打开{" "}
            {app.webUrl}
          </p>
        </div>
      </div>
    );
  }

  return (
    <div ref={hostRef} className="flex h-full items-center justify-center px-10">
      <div className="max-w-md text-center">
        <p className="text-xs font-semibold uppercase tracking-[0.18em] text-shell-accent">
          独立分区
        </p>
        <h1 className="mt-3 text-2xl font-semibold tracking-tight">{app.name}</h1>
        <p className="mt-3 text-sm leading-6 text-shell-muted">
          {error ?? "分区页面盖在这块区域上。登录窗口会单独弹出，并使用同一个分区。"}
        </p>
      </div>
    </div>
  );
}
