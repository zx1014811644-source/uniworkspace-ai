import { useState } from "react";
import { appsByKind } from "../catalog";
import { errorText, probePartitions, type ProbeReport } from "../partitions";
import { useShellStore } from "../store";
import type { CatalogApp } from "../types";
import { runningInTauri } from "../window";

export function Sidebar() {
  const webApps = appsByKind("web");
  const nativeApps = appsByKind("native");

  return (
    <aside className="flex w-64 shrink-0 flex-col border-r border-shell-line bg-shell-side">
      <div className="px-4 pb-2 pt-4">
        <p className="text-[11px] font-semibold uppercase tracking-[0.16em] text-shell-muted">
          应用
        </p>
        <p className="mt-1 text-xs leading-5 text-shell-muted">
          前四个网页会在右侧分区里打开。
        </p>
      </div>
      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-2 pb-4">
        <AppGroup title="网页" apps={webApps} />
        <AppGroup title="桌面" apps={nativeApps} />
      </div>
      <IsolationCheck />
    </aside>
  );
}

function IsolationCheck() {
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<ProbeReport | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  return (
    <div className="border-t border-shell-line px-3 py-3">
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
        {busy ? "正在检查四个分区…" : "检查隔离"}
      </button>
      {failure ? (
        <p className="mt-2 text-[11px] leading-5 text-[#8a3d32]">{failure}</p>
      ) : null}
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

function AppGroup({ title, apps }: { title: string; apps: CatalogApp[] }) {
  return (
    <section>
      <h2 className="px-2 pb-1 text-[11px] font-medium text-shell-muted">
        {title}
      </h2>
      <ul className="space-y-0.5">
        {apps.map((app) => (
          <li key={app.id}>
            <AppRow app={app} />
          </li>
        ))}
      </ul>
    </section>
  );
}

function AppRow({ app }: { app: CatalogApp }) {
  const selectedId = useShellStore((state) => state.selectedId);
  const select = useShellStore((state) => state.select);
  const selected = selectedId === app.id;
  const pending =
    (app.kind === "web" && app.webStatus === "pending") ||
    (app.kind === "native" && app.bundleIdStatus === "pending");

  return (
    <button
      type="button"
      aria-current={selected ? "true" : undefined}
      className={`flex w-full items-center justify-between rounded-md px-2 py-1.5 text-left text-sm ${
        selected
          ? "bg-white text-shell-ink shadow-sm"
          : "text-shell-ink hover:bg-black/5"
      }`}
      onClick={() => select(app.id)}
    >
      <span className="truncate">{app.name}</span>
      {pending ? (
        <span className="ml-2 shrink-0 text-[10px] text-shell-muted">待确认</span>
      ) : null}
    </button>
  );
}
