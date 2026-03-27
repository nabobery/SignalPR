import { BrowserRouter, Routes, Route } from "react-router";
import { ErrorBoundary } from "react-error-boundary";
import { IntakeView } from "./features/intake/IntakeView";
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
          <Route path="/" element={<IntakeView />} />
          <Route path="/review/:runId" element={<ReviewWorkspace />} />
          <Route path="/settings" element={<SettingsView />} />
        </Routes>
      </BrowserRouter>
    </ErrorBoundary>
  );
}

export default App;
