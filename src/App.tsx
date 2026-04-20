import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { BootWizard } from "./components/BootWizard";
import { GlobalSchedulerView } from "./components/GlobalSchedulerView";
import { ShipCard } from "./components/ShipCard";
import type { ShipInfo } from "./components/ShipCard";
import "./App.css";

function App() {
  const [ships, setShips]       = useState<ShipInfo[]>([]);
  const [logsByPier, setLogsByPier] = useState<Record<string, string[]>>({});
  const [showWizard, setShowWizard] = useState(false);
  const [activeView, setActiveView] = useState<"ships" | "scheduler">("ships");

  useEffect(() => {
    fetchShips();

    const cleanups = [
      listen("ship-ready",  () => fetchShips()),
      listen("ship-exited", () => fetchShips()),
      listen("ship-code",   () => fetchShips()),
      listen<{ line: string; pier_path?: string }>("ship-log", e => {
        const pierPath = e.payload.pier_path;
        if (!pierPath) return;

        setLogsByPier(prev => ({
          ...prev,
          [pierPath]: [...(prev[pierPath] ?? []).slice(-300), e.payload.line],
        }));
      }),
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
          <h1>🧑‍✈️ Portmate</h1>
          <span className="ship-count">{ships.length} ship{ships.length !== 1 ? "s" : ""}</span>
          <nav className="view-switcher" aria-label="Primary views">
            <button
              className={activeView === "ships" ? "view-button is-active" : "view-button"}
              onClick={() => setActiveView("ships")}
            >
              Ships
            </button>
            <button
              className={activeView === "scheduler" ? "view-button is-active" : "view-button"}
              onClick={() => setActiveView("scheduler")}
            >
              Scheduler
            </button>
          </nav>
        </div>
        <button className="boot-button" onClick={() => setShowWizard(true)}>
          + Boot New Ship
        </button>
      </header>

      <main className={activeView === "ships" ? "ships-grid" : "scheduler-main"}>
        {activeView === "ships" ? (
          ships.length === 0 ? (
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
                logs={logsByPier[ship.pierPath] ?? []}
                onStop={() => handleStop(ship.pierPath)}
                onRestart={() => handleRestart(ship.pierPath)}
                onDelete={() => handleDelete(ship.pierPath)}
              />
            ))
          )
        ) : (
          <GlobalSchedulerView ships={ships} />
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