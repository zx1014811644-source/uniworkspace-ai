import {
  closeWindow,
  minimizeWindow,
  toggleMaximizeWindow,
} from "../window";

export function TitleBar() {
  return (
    <header
      className="flex h-11 shrink-0 items-center gap-3 border-b border-shell-line bg-shell-bar px-3"
      data-tauri-drag-region
      onDoubleClick={() => {
        void toggleMaximizeWindow();
      }}
    >
      <div className="flex items-center gap-2">
        <WindowButton
          label="关闭"
          className="bg-[#c4554a] hover:bg-[#a93d34]"
          onClick={() => {
            void closeWindow();
          }}
        />
        <WindowButton
          label="最小化"
          className="bg-[#d5a03a] hover:bg-[#b88828]"
          onClick={() => {
            void minimizeWindow();
          }}
        />
        <WindowButton
          label="缩放"
          className="bg-[#6f9a4e] hover:bg-[#5a8140]"
          onClick={() => {
            void toggleMaximizeWindow();
          }}
        />
      </div>
      <div className="flex min-w-0 flex-1 items-center" data-tauri-drag-region>
        <p className="truncate text-[13px] font-medium tracking-wide">
          UniWorkspace
        </p>
      </div>
    </header>
  );
}

function WindowButton({
  label,
  className,
  onClick,
}: {
  label: string;
  className: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className={`h-3 w-3 rounded-full ${className}`}
      onMouseDown={(event) => event.stopPropagation()}
      onDoubleClick={(event) => event.stopPropagation()}
      onClick={onClick}
    />
  );
}
