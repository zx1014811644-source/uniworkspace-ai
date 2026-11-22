import { useState } from "react";
import { appsByKind, catalog } from "../catalog";
import { DesktopPanels } from "./DesktopPanels";
import { errorText, probePartitions, type ProbeReport } from "../partitions";
import { useShellStore } from "../store";
import type { CatalogApp } from "../types";
import { runningInTauri } from "../window";
import { officialWeb, pendingWeb, visibleCount } from "../workspace";

export function Sidebar() {
  const webApps = appsByKind("web");
  const customApps = useShellStore((state) => state.customApps);
  const inspectedId = useShellStore((state) => state.inspectedId);
  const inspected = catalog.apps.find((app) => app.id === inspectedId) ?? null;

  return (
    <aside className="flex h-full min-h-0 w-64 shrink-0 flex-col overflow-hidden border-r border-shell-line bg-shell-side">
      <div className="shrink-0 px-4 pb-2 pt-4">
        <p className="text-[11px] font-semibold uppercase tracking-[0.16em] text-shell-muted">
          应用
        </p>
        <p className="mt-1 text-xs leading-5 text-shell-muted">
          先点右侧一格，再点已确认的网页。待确认的不会打开。
        </p>
      </div>
      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-2 pb-4">
        <section>
          <h2 className="px-2 pb-1 text-[11px] font-medium text-shell-muted">网页</h2>
          <ul className="space-y-0.5">
            {webApps.map((app) => (
              <li key={app.id}>
                <CatalogRow app={app} />
              </li>
            ))}
          </ul>
        </section>
        <section>
          <h2 className="px-2 pb-1 text-[11px] font-medium text-shell-muted">自定义</h2>
          {customApps.length === 0 ? (
            <p className="px-2 text-xs leading-5 text-shell-muted">还没有手动添加的链接。</p>
          ) : (
            <ul className="space-y-0.5">
              {customApps.map((app) => (
                <li key={app.id}>
                  <CustomRow id={app.id} name={app.name} />
                </li>
              ))}
            </ul>
          )}
        </section>
        <DesktopPanels />
        {inspected ? <Inspected app={inspected} /> : null}
      </div>
      <CustomUrlForm />
      <IsolationCheck />
    </aside>
  );
}

function CatalogRow({ app }: { app: CatalogApp }) {
  const slots = useShellStore((state) => state.slots);
  const layout = useShellStore((state) => state.layout);
  const assignApp = useShellStore((state) => state.assignApp);
  const inspect = useShellStore((state) => state.inspect);
  const inspectedId = useShellStore((state) => state.inspectedId);
  const placed = slots.slice(0, visibleCount(layout)).includes(app.id);
  const blocked = pendingWeb(app) || app.kind === "native";
  const selected = blocked ? inspectedId === app.id : placed;

  return (
    <button
      type="button"
      aria-current={selected ? "true" : undefined}
      className={`flex w-full items-center justify-between rounded-md px-2 py-1.5 text-left text-sm ${
        selected ? "bg-white text-shell-ink shadow-sm" : "text-shell-ink hover:bg-black/5"
      }`}
      onClick={() => {
        if (officialWeb(app)) {
          void assignApp(app.id);
          return;
        }
        inspect(app.id);
      }}
    >
      <span className="truncate">{app.name}</span>
      {pendingWeb(app) ? (
        <span className="ml-2 shrink-0 text-[10px] text-shell-muted">不打开</span>
      ) : app.kind === "native" && app.bundleIdStatus === "pending" ? (
        <span className="ml-2 shrink-0 text-[10px] text-shell-muted">待确认</span>
      ) : null}
    </button>
  );
}

function CustomRow({ id, name }: { id: string; name: string }) {
  const slots = useShellStore((state) => state.slots);
  const layout = useShellStore((state) => state.layout);
  const assignApp = useShellStore((state) => state.assignApp);
  const removeCustom = useShellStore((state) => state.removeCustom);
  const placed = slots.slice(0, visibleCount(layout)).includes(id);

  return (
    <div
      className={`flex items-center rounded-md pr-1 ${
        placed ? "bg-white shadow-sm" : "hover:bg-black/5"
      }`}
    >
      <button
        type="button"
        className="min-w-0 flex-1 truncate px-2 py-1.5 text-left text-sm"
        onClick={() => {
          void assignApp(id);
        }}
      >
        {name}
      </button>
      <button
        type="button"
        className="shrink-0 rounded px-1.5 py-1 text-[10px] text-shell-muted hover:text-shell-ink"
        onClick={() => {
          void removeCustom(id);
        }}
      >
        移除
      </button>
    </div>
  );
}

function Inspected({ app }: { app: CatalogApp }) {
  let text = "这一周只吸附 Claude Desktop。这个应用只检测，不移动窗口。";
  if (app.id === "claude-desktop") {
    text =
      "先点右侧一格，再点运行中的「吸附」。窗口对齐到这一格，拆卸时还原边框。指定窗口仍然只列出，不移动。";
  } else if (pendingWeb(app)) {
    text = "没有已确认的官方网页入口，所以不会打开 WebView，也不会猜一个地址。这不阻挡其他网页。";
  } else if (app.kind === "native" && app.bundleIdStatus === "pending") {
    text = "Bundle ID 还没有核对，不会把它当成已检测到的桌面应用，也不会猜一个标识。";
  }

  return (
    <div className="mx-2 rounded-md border border-shell-line bg-white px-3 py-2">
      <p className="text-sm font-medium">{app.name}</p>
      {app.kind === "native" && app.bundleId ? (
        <p className="mt-1 break-all text-[11px] text-shell-muted">{app.bundleId}</p>
      ) : null}
      <p className="mt-1 text-[11px] leading-5 text-shell-muted">{text}</p>
    </div>
  );
}

function CustomUrlForm() {
  const [value, setValue] = useState("");
  const addCustomUrl = useShellStore((state) => state.addCustomUrl);
  const loadError = useShellStore((state) => state.loadError);
  const [busy, setBusy] = useState(false);

  return (
    <form
      className="shrink-0 border-t border-shell-line px-3 py-3"
      onSubmit={(event) => {
        event.preventDefault();
        if (!value.trim() || busy) {
          return;
        }
        setBusy(true);
        void addCustomUrl(value)
          .then(() => {
            if (!useShellStore.getState().loadError) {
              setValue("");
            }
          })
          .finally(() => {
            setBusy(false);
          });
      }}
    >
      <label className="block text-[11px] font-medium text-shell-muted" htmlFor="custom-url">
        自定义链接
      </label>
      <input
        id="custom-url"
        type="url"
        value={value}
        placeholder="https://"
        className="mt-1 w-full rounded-md border border-shell-line bg-white px-2 py-1.5 text-sm outline-none focus:border-shell-accent"
        onChange={(event) => setValue(event.target.value)}
      />
      <button
        type="submit"
        disabled={busy || !value.trim()}
        className="mt-2 w-full rounded-md bg-shell-accent px-2 py-1.5 text-xs text-white disabled:opacity-50"
      >
        {busy ? "正在添加…" : "添加到独立分区"}
      </button>
      {loadError ? <p className="mt-2 text-[11px] leading-5 text-[#8a3d32]">{loadError}</p> : null}
    </form>
  );
}

function IsolationCheck() {
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<ProbeReport | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  return (
    <div className="shrink-0 border-t border-shell-line px-3 py-3">
      <button
        type="button"
        disabled={busy}
        className="w-full rounded-md px-2 py-1.5 text-left text-xs text-shell-muted hover:bg-black/5 hover:text-shell-ink disabled:opacity-60"
        onClick={() => {
          if (!runningInTauri()) {
            setReport(null);
            setFailure("隔离检查要在 macOS 应用里运行。");
            return;
          }
          setBusy(true);
          setFailure(null);
          void probePartitions()
            .then((next) => {
              setReport(next);
            })
            .catch((reason: unknown) => {
              setReport(null);
              setFailure(errorText(reason));
            })
            .finally(() => {
              setBusy(false);
            });
        }}
      >
        {busy ? "正在检查已确认的网页分区…" : "检查隔离"}
      </button>
      {failure ? <p className="mt-2 text-[11px] leading-5 text-[#8a3d32]">{failure}</p> : null}
      {report ? <ProbeSummary report={report} /> : null}
    </div>
  );
}

function ProbeSummary({ report }: { report: ProbeReport }) {
  return (
    <div className="mt-2 max-h-40 overflow-y-auto text-[11px] leading-5 text-shell-muted">
      <p className={report.isolated ? "text-shell-accent" : "text-[#8a3d32]"}>
        {report.isolated ? "通过。" : "未通过。"}
        {report.detail}
      </p>
      <p className="mt-1">{report.backend}</p>
      <ul className="mt-1 space-y-1">
        {report.samples.map((sample) => (
          <li key={sample.appId}>
            {sample.appId} · localStorage {sample.localStorage || "空"} · cookie{" "}
            {sample.cookie || "空"}
          </li>
        ))}
      </ul>
      {report.hints.length > 0 ? (
        <ul className="mt-1 space-y-1">
          {report.hints.map((hint) => (
            <li key={hint}>{hint}</li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
