import { catalog } from "../catalog";
import { useShellStore } from "../store";

export function ContentArea() {
  const selectedId = useShellStore((state) => state.selectedId);
  const selected = catalog.apps.find((app) => app.id === selectedId) ?? null;

  return (
    <main className="min-w-0 flex-1 overflow-auto bg-shell-canvas">
      {selected ? <SelectedApp /> : <EmptyState />}
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
        <h1 className="mt-3 text-2xl font-semibold tracking-tight">
          这里还是空的
        </h1>
        <p className="mt-3 text-sm leading-6 text-shell-muted">
          从左侧点一个名字，只标出位置。这里还不会打开网页或桌面程序。
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
        ? "已记录官方网址，尚未打开"
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
