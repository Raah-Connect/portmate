import { useState, useEffect } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { TerminalOutput } from "./TerminalOutput";

export interface ShipInfo {
  name: string;
  pierPath: string;
  url: string;
  accessCode: string;
  status: "booting" | "running" | "stopped";
  binaryPath: string;
  pid: number | null;
}

interface Props {
  ship: ShipInfo;
  logs: string[];
  onStop: () => void;
  onRestart: () => void;
  onDelete: () => void;
}

export function ShipCard({ ship, logs, onStop, onRestart, onDelete }: Props) {
  const [expanded, setExpanded]     = useState(false);
  const [codeCopied, setCodeCopied] = useState(false);
  const [confirming, setConfirming] = useState<"stop" | "delete" | null>(null);

  function copyCode() {
    if (!ship.accessCode) return;
    navigator.clipboard.writeText(ship.accessCode);
    setCodeCopied(true);
    setTimeout(() => setCodeCopied(false), 15000);
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

  return (
    <div style={cardStyle}>
      {/* ── Header ── */}
      <div style={cardHeaderStyle}>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ fontSize: 20 }}>🚢</span>
          <div>
            <div style={{ fontWeight: 700, fontSize: 15, color: "#f1f5f9" }}>
              {ship.name}
            </div>
            <div style={{ fontSize: 11, color: "#64748b", fontFamily: "monospace" }}>
              {ship.pierPath}
            </div>
          </div>
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ fontSize: 12, color: statusColor, fontWeight: 600 }}>
            {statusLabel}
          </span>
          <button
            onClick={() => setExpanded(e => !e)}
            style={iconBtnStyle}
            title={expanded ? "Collapse" : "Expand"}
          >
            {expanded ? "▲" : "▼"}
          </button>
        </div>
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

            <button
                onClick={copyCode}
                style={ship.accessCode ? codeBtnStyle : codeEmptyBtnStyle}
                title={ship.accessCode ? "Click to copy access code" : "Access code not yet available"}
                disabled={!ship.accessCode}
            >
                <span style={{ fontFamily: "monospace", fontSize: 12 }}>
                {codeCopied ? ship.accessCode : "🔑 Access Code"}
                </span>
            </button>
            </div>

      {/* ── Terminal (expanded) ── */}
      {expanded && (
        <div style={{ padding: "0 16px 12px" }}>
          <TerminalOutput logs={logs} />
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

const iconBtnStyle: React.CSSProperties = {
  background: "transparent",
  border: "none",
  color: "#475569",
  cursor: "pointer",
  fontSize: 10,
  padding: "4px 6px",
};

const codeEmptyBtnStyle: React.CSSProperties = {
  ...codeBtnStyle,
  opacity: 0.4,
  cursor: "not-allowed",
};