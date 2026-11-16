import { useEffect } from "react";
import { ContentArea } from "./components/ContentArea";
import { Sidebar } from "./components/Sidebar";
import { TitleBar } from "./components/TitleBar";
import { desktopScan, scanWait } from "./desktop";
import { errorText, listenForPaneNotices } from "./partitions";
import { listenForSnapEnded } from "./snap";
import { useShellStore } from "./store";
import { runningInTauri } from "./window";

export default function App() {
  const boot = useShellStore((state) => state.boot);
  const notePane = useShellStore((state) => state.notePane);

  useEffect(() => {
    void boot();
  }, [boot]);

  useEffect(() => {
    if (!runningInTauri()) {
      return;
    }
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listenForPaneNotices(
      (notice) => notePane(notice.appId, notice.message),
      (notice) => notePane(notice.appId, notice.message),
    ).then((stop) => {
      if (cancelled) {
        stop();
        return;
      }
      unlisten = stop;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [notePane]);

  useEffect(() => {
    if (!runningInTauri()) {
      return;
    }
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listenForSnapEnded((status) => {
      useShellStore.getState().applySnapEnded(status);
    }).then((stop) => {
      if (cancelled) {
        stop();
        return;
      }
      unlisten = stop;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!runningInTauri()) {
      return;
    }
    let stopped = false;
    let timer = 0;
    const loop = () => {
      void desktopScan()
        .then((scan) => {
          if (stopped) {
            return;
          }
          useShellStore.getState().applyScan(scan);
          timer = window.setTimeout(loop, scanWait(scan.intervalMs));
        })
        .catch((reason: unknown) => {
          if (stopped) {
            return;
          }
          const message = errorText(reason);
          if (!message.includes("上一次检测")) {
            useShellStore.getState().setScanError(message);
          }
          timer = window.setTimeout(loop, 1000);
        });
    };
    loop();
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, []);

  return (
    <div className="flex h-full flex-col bg-shell-frame text-shell-ink">
      <TitleBar />
      <div className="flex min-h-0 flex-1">
        <Sidebar />
        <ContentArea />
      </div>
    </div>
  );
}
