import { BrowserRouter, Routes, Route } from "react-router";
import { IntakeView } from "./features/intake/IntakeView";
import { ReviewWorkspace } from "./features/review/ReviewWorkspace";
import "./App.css";

function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<IntakeView />} />
        <Route path="/review/:runId" element={<ReviewWorkspace />} />
      </Routes>
    </BrowserRouter>
  );
}

export default App;
