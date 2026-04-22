import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { TerminalOutput } from "./TerminalOutput";
import { MemoryManager } from "./MemoryManager";
import { MemorySchedular } from "./MemorySchedular";

export interface ShipInfo {
  name: string;
  pierPath: string;
  url: string;
  accessCode: string;
  status: "booting" | "running" | "stopped";
  binaryPath: string;
  pid: number | null;
  pierSizeBytes?: number | null;
}

interface ShipSizeUpdatedPayload {
  pierPath: string;
  pierSizeBytes: number;
}

interface Props {
  ship: ShipInfo;
  logs: string[];
  onStop: () => void;
  onRestart: () => void;
  onDelete: () => void;
}

type Panel = "terminal" | "memoryManager" | "memoryScheduler";

export function ShipCard({ ship, logs, onStop, onRestart, onDelete }: Props) {
  const [activePanel, setActivePanel] = useState<Panel | null>(null);
  const [codeCopied, setCodeCopied]   = useState(false);
  const [codeLoading, setCodeLoading] = useState(false);
  const [accessCodeError, setAccessCodeError] = useState("");
  const [confirming, setConfirming]   = useState<"stop" | "delete" | null>(null);
  const [pierSizeBytes, setPierSizeBytes] = useState<number | null>(ship.pierSizeBytes ?? null);
  const [sizeRefreshing, setSizeRefreshing] = useState(false);

  useEffect(() => {
    setPierSizeBytes(ship.pierSizeBytes ?? null);
  }, [ship.pierSizeBytes, ship.pierPath]);

  useEffect(() => {
    if (ship.accessCode) {
      setCodeLoading(false);
      setAccessCodeError("");
    }
  }, [ship.accessCode]);

  useEffect(() => {
    if (ship.status !== "running") {
      setCodeLoading(false);
      setAccessCodeError("");
    }
  }, [ship.status, ship.pierPath]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    listen<ShipSizeUpdatedPayload>("ship-size-updated", (event) => {
      if (event.payload.pierPath !== ship.pierPath) return;
      setPierSizeBytes(event.payload.pierSizeBytes);
      setSizeRefreshing(false);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      unlisten?.();
    };
  }, [ship.pierPath]);

  function togglePanel(panel: Panel) {
    setActivePanel(prev => prev === panel ? null : panel);
  }

  function copyCode() {
    if (!ship.accessCode) return;
    navigator.clipboard.writeText(ship.accessCode);
    setCodeCopied(true);
    setTimeout(() => setCodeCopied(false), 15000);
  }

  async function requestAccessCode() {
    if (ship.status !== "running" || ship.accessCode || codeLoading) return;

    setAccessCodeError("");
    setCodeLoading(true);
    try {
      await invoke("request_access_code", { pierPath: ship.pierPath });
    } catch (error) {
      console.error(error);
      setAccessCodeError(String(error));
      setCodeLoading(false);
    }
  }

  function confirmThen(action: "stop" | "delete") {
    if (confirming === action) {
      if (action === "stop") onStop();
      if (action === "delete") onDelete();
      setConfirming(null);
    } else {
      setConfirming(action);
      setTimeout(() => setConfirming(null), 3000);
    }
  }

  async function refreshPierSize() {
    setSizeRefreshing(true);
    try {
      const size = await invoke<number>("refresh_ship_size_command", {
        pierPath: ship.pierPath,
      });
      setPierSizeBytes(size);
    } catch {
      // Keep UX quiet for now; periodic refreshes and event updates still apply.
    } finally {
      setSizeRefreshing(false);
    }
  }

  const statusColor = ({
    booting: "#f59e0b",
    running: "#10b981",
    stopped: "#6b7280",
  } as Record<string, string>)[ship.status] ?? "#6b7280";

  const statusLabel = ({
    booting: "⏳ Booting",
    running: "● Running",
    stopped: "○ Stopped",
  } as Record<string, string>)[ship.status] ?? ship.status;
  const showGetAccessCode = ship.status === "running" && !ship.accessCode;

  return (
    <div style={cardStyle}>
      {/* ── Header ── */}
      <div style={cardHeaderStyle}>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ fontSize: 20 }}>🛸</span>
          <div>
            <div style={{ fontWeight: 700, fontSize: 15, color: "#f1f5f9" }}>
              {ship.name}
            </div>
            <div style={{ fontSize: 11, color: "#64748b", fontFamily: "monospace" }}>
              {ship.pierPath}
            </div>
          </div>
        </div>
        <span style={{ fontSize: 12, color: statusColor, fontWeight: 600 }}>
          {statusLabel}
        </span>
      </div>

      {/* ── Info Row ── */}
      <div style={infoRowStyle}>
        {ship.url ? (
          <button onClick={() => openUrl(ship.url)} style={landscapeBtnStyle}>
            Open Landscape →
          </button>
        ) : (
          <span style={{ fontSize: 12, color: "#475569" }}>
            {ship.status === "booting" ? "Waiting for web interface…" : "Not running"}
          </span>
        )}
        <div style={sizeWrapStyle}>
          <span style={sizeLabelStyle}>
            Pier size {pierSizeBytes == null ? "--" : formatBytes(pierSizeBytes)}
          </span>
          <button
            onClick={refreshPierSize}
            disabled={sizeRefreshing}
            style={sizeRefreshBtnStyle(sizeRefreshing)}
            title="Refresh pier size"
          >
            {sizeRefreshing ? "Refreshing..." : "Refresh"}
          </button>
        </div>
        <button
          onClick={showGetAccessCode ? () => void requestAccessCode() : copyCode}
          style={showGetAccessCode ? codeActionBtnStyle : ship.accessCode ? codeBtnStyle : codeEmptyBtnStyle}
          title={ship.accessCode ? "Click to copy access code" : showGetAccessCode ? "Request the access code from dojo" : "Access code not yet available"}
          disabled={showGetAccessCode ? codeLoading : !ship.accessCode}
        >
          <span style={{ fontFamily: "monospace", fontSize: 12 }}>
            {showGetAccessCode ? codeLoading ? "Getting..." : "Get Access Code" : codeCopied ? ship.accessCode : "🔑 Access Code"}
          </span>
        </button>
      </div>

      {accessCodeError && (
        <div style={errorBannerStyle}>
          {accessCodeError}
        </div>
      )}

      {/* ── Tab Bar ── */}
      <div style={tabBarStyle}>
        <button
          onClick={() => togglePanel("terminal")}
          style={tabBtnStyle(activePanel === "terminal")}
        >
          Terminal {activePanel === "terminal" ? "▲" : "▼"}
        </button>
        <button
          onClick={() => togglePanel("memoryManager")}
          style={tabBtnStyle(activePanel === "memoryManager", "#818cf8", "#1e1b4b", "#312e81")}
        >
          Memory Ops {activePanel === "memoryManager" ? "▲" : "▼"}
        </button>
        <button
          onClick={() => togglePanel("memoryScheduler")}
          style={tabBtnStyle(activePanel === "memoryScheduler", "#60a5fa", "#0b2244", "#1d4ed8")}
        >
          Maintenance Scheduler {activePanel === "memoryScheduler" ? "▲" : "▼"}
        </button>
      </div>

      {/* ── Panel: Terminal ── */}
      {activePanel === "terminal" && (
        <div style={panelStyle}>
          <TerminalOutput logs={logs} />
        </div>
      )}

      {/* ── Panel: Memory Manager ── */}
      {activePanel === "memoryManager" && (
        <div style={memoryPanelStyle}>
          <MemoryManager ship={ship} />
        </div>
      )}

      {/* ── Panel: Memory Scheduler ── */}
      {activePanel === "memoryScheduler" && (
        <div style={schedulerPanelStyle}>
          <MemorySchedular ship={ship} />
        </div>
      )}

      {/* ── Actions ── */}
      <div style={actionsStyle}>
        {ship.status === "running" && (
          <button
            onClick={() => confirmThen("stop")}
            style={confirming === "stop" ? dangerActiveBtnStyle : dangerBtnStyle}
          >
            {confirming === "stop" ? "Click again to stop" : "Stop"}
          </button>
        )}
        {ship.status === "stopped" && (
          <button onClick={onRestart} style={actionBtnStyle}>
            Restart
          </button>
        )}
        <button
          onClick={() => confirmThen("delete")}
          style={confirming === "delete" ? dangerActiveBtnStyle : ghostBtnStyle}
        >
          {confirming === "delete" ? "Click again to delete" : "Delete pier"}
        </button>
        {ship.pid && (
          <span style={{ fontSize: 11, color: "#475569", marginLeft: "auto" }}>
            PID {ship.pid}
          </span>
        )}
      </div>
    </div>
  );
}

function formatBytes(value: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = value;
  let unit = 0;

  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }

  const precision = unit <= 1 ? 0 : 1;
  return `${size.toFixed(precision)} ${units[unit]}`;
}

// ── Styles ────────────────────────────────────────────────────────────────────

const cardStyle: React.CSSProperties = {
  background: "#0f172a",
  border: "1px solid #1e293b",
  borderRadius: 10,
  overflow: "hidden",
  transition: "border-color 0.2s",
};

const cardHeaderStyle: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "center",
  padding: "14px 16px",
  borderBottom: "1px solid #1e293b",
};

const infoRowStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 10,
  padding: "10px 16px",
  borderBottom: "1px solid #1e293b",
  minHeight: 44,
  flexWrap: "wrap",
};

const errorBannerStyle: React.CSSProperties = {
  margin: "10px 16px 0",
  borderRadius: 8,
  border: "1px solid #7f1d1d",
  background: "#2b0b0b",
  color: "#fecaca",
  fontSize: 12,
  lineHeight: 1.45,
  padding: "8px 10px",
};

const sizeWrapStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 6,
  marginLeft: "auto",
};

const sizeLabelStyle: React.CSSProperties = {
  fontSize: 11,
  color: "#94a3b8",
  fontFamily: "monospace",
};

function sizeRefreshBtnStyle(disabled: boolean): React.CSSProperties {
  return {
    background: disabled ? "#111827" : "#1e293b",
    color: disabled ? "#64748b" : "#cbd5e1",
    border: "1px solid #334155",
    borderRadius: 6,
    padding: "4px 8px",
    fontSize: 11,
    cursor: disabled ? "not-allowed" : "pointer",
  };
}

const tabBarStyle: React.CSSProperties = {
  display: "flex",
  gap: 6,
  padding: "8px 16px",
  borderBottom: "1px solid #1e293b",
  background: "#080d16",
};

function tabBtnStyle(
  active: boolean,
  activeColor  = "#94a3b8",
  activeBg     = "#1e293b",
  activeBorder = "#334155",
): React.CSSProperties {
  return {
    background:   active ? activeBg      : "transparent",
    color:        active ? activeColor   : "#475569",
    border:       `1px solid ${active ? activeBorder : "transparent"}`,
    borderRadius: 6,
    padding:      "5px 12px",
    fontSize:     11,
    fontWeight:   600,
    cursor:       "pointer",
    fontFamily:   "inherit",
    transition:   "background 0.15s, color 0.15s",
  };
}

const panelStyle: React.CSSProperties = {
  padding: "12px 16px",
  borderBottom: "1px solid #1e293b",
};

const memoryPanelStyle: React.CSSProperties = {
  borderBottom: "1px solid #1e293b",
};

const schedulerPanelStyle: React.CSSProperties = {
  borderBottom: "1px solid #1e293b",
  maxHeight: 520,
  overflow: "auto",
};

const actionsStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "10px 16px",
};

const landscapeBtnStyle: React.CSSProperties = {
  background: "#0070f3",
  color: "#fff",
  border: "none",
  borderRadius: 6,
  padding: "6px 14px",
  fontSize: 12,
  fontWeight: 600,
  cursor: "pointer",
};

const codeBtnStyle: React.CSSProperties = {
  background: "#022c22",
  color: "#10b981",
  border: "1px solid #064e3b",
  borderRadius: 6,
  padding: "5px 10px",
  cursor: "pointer",
  marginLeft: "auto",
};

const codeEmptyBtnStyle: React.CSSProperties = {
  ...codeBtnStyle,
  opacity: 0.4,
  cursor: "not-allowed",
};

const codeActionBtnStyle: React.CSSProperties = {
  ...codeBtnStyle,
  background: "#0b2545",
  color: "#bfdbfe",
  border: "1px solid #1d4ed8",
};

const actionBtnStyle: React.CSSProperties = {
  background: "#1e293b",
  color: "#94a3b8",
  border: "1px solid #334155",
  borderRadius: 6,
  padding: "6px 14px",
  fontSize: 12,
  cursor: "pointer",
};

const dangerBtnStyle: React.CSSProperties = {
  ...actionBtnStyle,
  color: "#f87171",
  borderColor: "#7f1d1d",
};

const dangerActiveBtnStyle: React.CSSProperties = {
  ...dangerBtnStyle,
  background: "#7f1d1d",
  color: "#fca5a5",
};

const ghostBtnStyle: React.CSSProperties = {
  ...actionBtnStyle,
  background: "transparent",
  color: "#475569",
  borderColor: "transparent",
};
