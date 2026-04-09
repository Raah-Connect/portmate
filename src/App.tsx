import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { BootWizard } from "./components/BootWizard";
import { ShipCard } from "./components/ShipCard";
import type { ShipInfo } from "./components/ShipCard";
import "./App.css";

function App() {
  const [ships, setShips]       = useState<ShipInfo[]>([]);
  const [logs, setLogs]         = useState<string[]>([]);
  const [showWizard, setShowWizard] = useState(false);

  useEffect(() => {
    fetchShips();

    const cleanups = [
      listen("ship-ready",  () => fetchShips()),
      listen("ship-exited", () => fetchShips()),
      listen("ship-code",   () => fetchShips()),
      listen<{ line: string }>("ship-log", e =>
        setLogs(prev => [...prev.slice(-300), e.payload.line])
      ),
    ];

    const interval = setInterval(fetchShips, 10000);

    return () => {
      cleanups.forEach(p => p.then(fn => fn()));
      clearInterval(interval);
    };
  }, []);

  async function fetchShips() {
    try {
      const result = await invoke<ShipInfo[]>("get_running_ships");
      setShips(result);
    } catch (e) {
      console.error("fetchShips failed:", e);
    }
  }

  async function handleStop(pierPath: string) {
    try { await invoke("stop_ship", { pierPath }); } catch (e) { console.error(e); }
    await fetchShips();
  }

  async function handleRestart(pierPath: string) {
    try { await invoke("restart_ship", { pierPath }); } catch (e) { console.error(e); }
    await fetchShips();
  }

  async function handleDelete(pierPath: string) {
    try { await invoke("delete_ship", { pierPath }); } catch (e) { console.error(e); }
    await fetchShips();
  }

  return (
    <div className="app-container">
      <header className="app-header">
        <div className="header-left">
          <h1> Portmate</h1>
          <span className="ship-count">{ships.length} ship{ships.length !== 1 ? "s" : ""}</span>
        </div>
        <button className="boot-button" onClick={() => setShowWizard(true)}>
          + Boot New Ship
        </button>
      </header>

      <main className="ships-grid">
        {ships.length === 0 ? (
          <div className="empty-state">
            <div className="empty-icon">🚢</div>
            <h2>No ships booted yet</h2>
            <p>Click "Boot New Ship" to create your first comet</p>
          </div>
        ) : (
          ships.map((ship, i) => (
            <ShipCard
              key={i}
              ship={ship}
              logs={logs}
              onStop={() => handleStop(ship.pierPath)}
              onRestart={() => handleRestart(ship.pierPath)}
              onDelete={() => handleDelete(ship.pierPath)}
            />
          ))
        )}
      </main>

      {showWizard && (
        <div className="modal-overlay" onClick={() => setShowWizard(false)}>
          <div className="modal-content" onClick={e => e.stopPropagation()}>
            <button className="modal-close" onClick={() => setShowWizard(false)}>✕</button>
            <BootWizard onComplete={() => { setShowWizard(false); fetchShips(); }} />
          </div>
        </div>
      )}
    </div>
  );
}

export default App;