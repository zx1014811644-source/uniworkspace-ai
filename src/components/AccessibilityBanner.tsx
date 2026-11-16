import { useState } from "react";
import { useShellStore } from "../store";

export function AccessibilityBanner() {
  const ready = useShellStore((state) => state.a11yReady);
  const open = useShellStore((state) => state.a11yOpen);
  const trusted = useShellStore((state) => state.a11yTrusted);
  const choice = useShellStore((state) => state.a11yChoice);
  const error = useShellStore((state) => state.a11yError);
  const requestA11y = useShellStore((state) => state.requestA11y);
  const openA11ySettings = useShellStore((state) => state.openA11ySettings);
  const refreshA11y = useShellStore((state) => state.refreshA11y);
  const chooseA11y = useShellStore((state) => state.chooseA11y);
  const hideA11y = useShellStore((state) => state.hideA11y);
  const [busy, setBusy] = useState<string | null>(null);

  if (!ready || !open) {
    return null;
  }

  const run = (name: string, action: () => Promise<void>) => {
    if (busy) {
      return;
    }
    setBusy(name);
    void action().finally(() => {
      setBusy(null);
    });
  };

  return (
    <section className="shrink-0 border-b border-shell-line bg-shell-side px-4 py-3">
      <h2 className="text-sm font-medium">辅助功能</h2>
      <p className="mt-1 max-w-3xl text-xs leading-5 text-shell-muted">
        把已确认的桌面应用吸附到分屏，以及列出屏幕上的窗口，需要 macOS
        的辅助功能权限（Level 2）。只有点「吸附」时才会移动那个窗口。网页分屏、登录和自定义链接不需要这项权限。拒绝或稍后授权时，工作台继续可用，也不会移动任何窗口。指定窗口只列出，不移动。
      </p>
      <p className="mt-1 text-xs text-shell-ink">
        当前：{trusted ? "已授权" : "未授权"}
        {choice === "granted" ? " · 已完成" : null}
        {choice === "webOnly" ? " · 仅使用网页" : null}
      </p>
      {!trusted ? (
        <p className="mt-1 max-w-3xl text-[11px] leading-5 text-shell-muted">
          如果系统对话框已经关掉，请到「系统设置 → 隐私与安全性 →
          辅助功能」打开本应用。未签名的调试构建换过路径后，开关可能要重新打开。
        </p>
      ) : null}
      <div className="mt-2 flex flex-wrap gap-2">
        <BannerButton
          label={busy === "request" ? "正在请求…" : "请求系统授权"}
          disabled={busy !== null}
          onClick={() => {
            run("request", requestA11y);
          }}
        />
        <BannerButton
          label="打开辅助功能设置"
          disabled={busy !== null}
          onClick={() => {
            run("settings", openA11ySettings);
          }}
        />
        <BannerButton
          label={busy === "refresh" ? "正在检查…" : "重新检查"}
          disabled={busy !== null}
          onClick={() => {
            run("refresh", refreshA11y);
          }}
        />
        {trusted && choice !== "granted" ? (
          <BannerButton
            label="完成"
            disabled={busy !== null}
            onClick={() => {
              run("grant", () => chooseA11y("granted"));
            }}
          />
        ) : null}
        {choice !== "webOnly" ? (
          <BannerButton
            label="仅使用网页"
            disabled={busy !== null}
            onClick={() => {
              run("web", () => chooseA11y("webOnly"));
            }}
          />
        ) : null}
        {choice !== "pending" ? (
          <BannerButton
            label="关闭"
            disabled={busy !== null}
            onClick={hideA11y}
          />
        ) : null}
      </div>
      {error ? <p className="mt-2 text-[11px] leading-5 text-[#8a3d32]">{error}</p> : null}
    </section>
  );
}

function BannerButton({
  label,
  disabled,
  onClick,
}: {
  label: string;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      className="rounded-md border border-shell-line bg-white px-2 py-1 text-xs text-shell-ink hover:bg-black/5 disabled:opacity-50"
      onClick={onClick}
    >
      {label}
    </button>
  );
}
