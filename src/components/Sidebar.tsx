import { appsByKind } from "../catalog";
import { useShellStore } from "../store";
import type { CatalogApp } from "../types";

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
          只标出位置，不会打开窗口。
        </p>
      </div>
      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-2 pb-4">
        <AppGroup title="网页" apps={webApps} />
        <AppGroup title="桌面" apps={nativeApps} />
      </div>
    </aside>
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
