import { useEffect, useState } from "react";
import { useShellStore } from "../store";

export function ShortcutControl() {
  const accelerator = useShellStore((state) => state.shortcutAccelerator);
  const registered = useShellStore((state) => state.shortcutRegistered);
  const error = useShellStore((state) => state.shortcutError);
  const setShortcut = useShellStore((state) => state.setShortcut);
  const suspendShortcut = useShellStore((state) => state.suspendShortcut);
  const resumeShortcut = useShellStore((state) => state.resumeShortcut);
  const [capturing, setCapturing] = useState(false);

  useEffect(() => {
    if (!capturing) {
      return;
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.repeat) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape" && !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey) {
        setCapturing(false);
        void resumeShortcut();
        return;
      }
      const chord = chordFromEvent(event);
      if (!chord) {
        return;
      }
      setCapturing(false);
      void setShortcut(chord);
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
    };
  }, [capturing, resumeShortcut, setShortcut]);

  return (
    <div className="flex min-w-0 items-center gap-1">
      <button
        type="button"
        className="truncate rounded-md px-2 py-1 text-xs text-shell-muted hover:bg-black/5 hover:text-shell-ink"
        title={error ?? (registered ? "全局热键已注册" : "全局热键还没有注册")}
        onClick={() => {
          if (capturing) {
            setCapturing(false);
            void resumeShortcut();
            return;
          }
          void suspendShortcut()
            .then(() => {
              setCapturing(true);
            })
            .catch(() => {
              setCapturing(false);
            });
        }}
      >
        {capturing ? "按下快捷键，Esc 取消" : `热键 ${accelerator}`}
      </button>
    </div>
  );
}

function chordFromEvent(event: KeyboardEvent): string | null {
  if (event.key === "Meta" || event.key === "Alt" || event.key === "Shift" || event.key === "Control") {
    return null;
  }
  const parts: string[] = [];
  if (event.ctrlKey) {
    parts.push("Control");
  }
  if (event.altKey) {
    parts.push("Option");
  }
  if (event.shiftKey) {
    parts.push("Shift");
  }
  if (event.metaKey) {
    parts.push("Command");
  }
  if (parts.length === 0) {
    return null;
  }
  let key: string | null = null;
  if (event.code === "Space" || event.key === " ") {
    key = "Space";
  } else if (/^Key[A-Z]$/.test(event.code)) {
    key = event.code.slice(3);
  } else if (/^Digit[0-9]$/.test(event.code)) {
    key = event.code.slice(5);
  }
  if (!key) {
    return null;
  }
  parts.push(key);
  return parts.join("+");
}
