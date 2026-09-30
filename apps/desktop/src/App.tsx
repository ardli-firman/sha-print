import { RuntimeStatusPanel } from "./components/RuntimeStatusPanel";
import { useRuntimeStatus } from "./hooks/useRuntimeStatus";
import "./App.css";

function App() {
  const runtime = useRuntimeStatus();

  return (
    <main className="app">
      <header className="app-header">
        <h1>ShaPrint</h1>
        <p className="tagline">Share printers and print through your normal Windows print dialog.</p>
      </header>

      <RuntimeStatusPanel runtime={runtime} />

      <footer className="app-footer">
        Documents are submitted by Windows, not by this window: ShaPrint never shows or logs print
        job content.
      </footer>
    </main>
  );
}

export default App;
