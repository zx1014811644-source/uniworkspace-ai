import { ContentArea } from "./components/ContentArea";
import { Sidebar } from "./components/Sidebar";
import { TitleBar } from "./components/TitleBar";

export default function App() {
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
