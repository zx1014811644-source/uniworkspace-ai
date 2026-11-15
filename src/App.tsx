import { useEffect } from "react";
import { ContentArea } from "./components/ContentArea";
import { Sidebar } from "./components/Sidebar";
import { TitleBar } from "./components/TitleBar";
import { listenForPaneNotices } from "./partitions";
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
