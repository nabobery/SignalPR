import { BrowserRouter, Routes, Route } from "react-router";
import { ErrorBoundary } from "react-error-boundary";
import { AppShell } from "./features/shell/AppShell";
import { InboxView } from "./features/inbox/InboxView";
import { ReviewWorkspace } from "./features/review/ReviewWorkspace";
import { SettingsView } from "./features/settings/SettingsView";
import "./App.css";

function ErrorFallback({
  error,
  resetErrorBoundary,
}: {
  error: unknown;
  resetErrorBoundary: () => void;
}) {
  const message = error instanceof Error ? error.message : String(error);
  return (
    <div className="min-h-screen bg-zinc-950 text-zinc-100 flex flex-col items-center justify-center gap-4">
      <p className="text-red-400">Something went wrong</p>
      <p className="text-zinc-400 text-sm">{message}</p>
      <button
        onClick={resetErrorBoundary}
        className="text-zinc-300 hover:text-zinc-100 text-sm underline"
      >
        Try again
      </button>
    </div>
  );
}

function App() {
  return (
    <ErrorBoundary FallbackComponent={ErrorFallback}>
      <BrowserRouter>
        <Routes>
          <Route element={<AppShell />}>
            <Route path="/" element={<InboxView />} />
            <Route path="/settings" element={<SettingsView />} />
          </Route>
          <Route path="/review/:runId" element={<ReviewWorkspace />} />
        </Routes>
      </BrowserRouter>
    </ErrorBoundary>
  );
}

export default App;
