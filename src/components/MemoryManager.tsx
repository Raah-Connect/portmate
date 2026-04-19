import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ShipInfo } from "./ShipCard";

// ── Types ─────────────────────────────────────────────────────────────────────

type OpId = "pack" | "meld" | "roll" | "chop";
type Phase = "idle" | "running" | "done" | "error";

interface MemoryOpDonePayload {
  pier_path: string;
  op: string;
  success: boolean;
  error?: string;
}

interface Props {
  ship: ShipInfo;
}

// ── Op definitions ────────────────────────────────────────────────────────────

const OPS: {
  id: OpId;
  label: string;
  command: string;
  desc: string;
  detail: string;
  color: string;
  bg: string;
  border: string;
}[] = [
  {
    id: "pack",
    label: "Pack",
    command: "pack_ship",
    desc: "Compact loom memory",
    detail: "Stops & restarts ship",
    color: "#f59e0b",
    bg: "#1c1107",
    border: "#451a03",
  },
  {
    id: "meld",
    label: "Meld",
    command: "meld_ship",
    desc: "Deduplicate loom data",
    detail: "Stops & restarts ship",
    color: "#f59e0b",
    bg: "#1c1107",
    border: "#451a03",
  },
  {
    id: "roll",
    label: "Roll",
    command: "roll_ship",
    desc: "Compact event log",
    detail: "Stops & restarts ship",
    color: "#818cf8",
    bg: "#0f0e1f",
    border: "#312e81",
  },
  {
    id: "chop",
    label: "Chop",
    command: "chop_ship",
    desc: "Trim old snapshots",
    detail: "Run after roll — stops & restarts",
    color: "#818cf8",
    bg: "#0f0e1f",
    border: "#312e81",
  },
];

// ── Component ─────────────────────────────────────────────────────────────────

export function MemoryManager({ ship }: Props) {
  const [phase, setPhase]       = useState<Phase>("idle");
  const [activeOp, setActiveOp] = useState<OpId | null>(null);
  const [message, setMessage]   = useState("");

  // Listen for completion events from the backend.
  useEffect(() => {
    let unlisten: (() => void) | undefined;

    listen<MemoryOpDonePayload>("memory-op-done", (event) => {
      if (event.payload.pier_path !== ship.pierPath) return;

      if (event.payload.success) {
        setPhase("done");
        setMessage(`${event.payload.op} complete - restart requested.`);
        setTimeout(() => {
          setPhase("idle");
          setActiveOp(null);
          setMessage("");
        }, 2500);
      } else {
        setPhase("error");
        setMessage(event.payload.error ?? "Operation failed");
        setTimeout(() => {
          setPhase("idle");
          setActiveOp(null);
        }, 6000);
      }
    }).then((fn) => { unlisten = fn; });

    return () => { unlisten?.(); };
  }, [ship.pierPath]);

  async function runOp(op: (typeof OPS)[number]) {
    setPhase("running");
    setActiveOp(op.id);
    setMessage(`Stopping ship and running ${op.label}…`);

    try {
      // invoke returns immediately after kicking off the background thread.
      // Completion arrives via the memory-op-done event above.
      await invoke(op.command, { pierPath: ship.pierPath });
    } catch (e) {
      setPhase("error");
      setMessage(String(e));
      setTimeout(() => {
        setPhase("idle");
        setActiveOp(null);
      }, 6000);
    }
  }

  const isBusy = phase === "running";

  return (
    <div style={wrapStyle}>
      {/* Section label */}
      <div style={sectionLabelStyle}>Memory Management</div>

      {/* Status banner */}
      {phase !== "idle" && (
        <div style={bannerStyle(phase)}>
          <span style={{ fontSize: 13 }}>
            {phase === "running" && "⟳ "}
            {phase === "done"    && "✓ "}
            {phase === "error"   && "✗ "}
          </span>
          {message}
        </div>
      )}

      {/* Op grid */}
      <div style={gridStyle}>
        {OPS.map((op) => {
          const isActive = activeOp === op.id;
          return (
            <div key={op.id} style={tileStyle(op, isActive, isBusy)}>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontSize: 13, fontWeight: 700, color: isActive ? op.color : "#e2e8f0" }}>
                  {op.label}
                </div>
                <div style={{ fontSize: 11, color: "#64748b", marginTop: 2 }}>
                  {op.desc}
                </div>
                <div style={{ fontSize: 10, color: "#334155", marginTop: 2 }}>
                  {op.detail}
                </div>
              </div>
              <button
                onClick={() => !isBusy && runOp(op)}
                disabled={isBusy}
                style={runBtnStyle(op, isActive, isBusy)}
              >
                {isActive && phase === "running" ? "…" : "Run"}
              </button>
            </div>
          );
        })}
      </div>

      {/* Hint */}
      <div style={hintStyle}>
        All ops stop the ship, run the binary, then restart automatically in the backend. Check the terminal for progress. Run chop after roll.
      </div>
    </div>
  );
}

// ── Styles ────────────────────────────────────────────────────────────────────

const wrapStyle: React.CSSProperties = {
  padding: "12px 16px",
  borderBottom: "1px solid #1e293b",
};

const sectionLabelStyle: React.CSSProperties = {
  fontSize: 10,
  fontWeight: 600,
  color: "#475569",
  letterSpacing: "0.08em",
  textTransform: "uppercase",
  marginBottom: 10,
};

function bannerStyle(phase: Phase): React.CSSProperties {
  const map: Record<Phase, { bg: string; border: string; color: string }> = {
    idle:    { bg: "transparent", border: "transparent", color: "transparent" },
    running: { bg: "#0c1a2e",     border: "#1e3a5f",     color: "#93c5fd"     },
    done:    { bg: "#022c22",     border: "#064e3b",     color: "#10b981"     },
    error:   { bg: "#1c0a0a",     border: "#7f1d1d",     color: "#f87171"     },
  };
  const t = map[phase];
  return {
    background:   t.bg,
    border:       `1px solid ${t.border}`,
    borderRadius: 6,
    padding:      "7px 12px",
    marginBottom: 10,
    fontSize:     12,
    color:        t.color,
    display:      "flex",
    alignItems:   "center",
    gap:          6,
  };
}

const gridStyle: React.CSSProperties = {
  display:             "grid",
  gridTemplateColumns: "1fr 1fr",
  gap:                 8,
};

function tileStyle(
  op: (typeof OPS)[number],
  isActive: boolean,
  isBusy: boolean,
): React.CSSProperties {
  return {
    background:   isActive ? op.bg : "#080d16",
    border:       `1px solid ${isActive ? op.border : "#1e293b"}`,
    borderRadius: 8,
    padding:      "10px 12px",
    display:      "flex",
    alignItems:   "center",
    gap:          10,
    opacity:      isBusy && !isActive ? 0.38 : 1,
    transition:   "background 0.2s, border-color 0.2s, opacity 0.2s",
  };
}

function runBtnStyle(
  op: (typeof OPS)[number],
  isActive: boolean,
  isBusy: boolean,
): React.CSSProperties {
  return {
    flexShrink:   0,
    background:   isActive ? op.bg : "#1e293b",
    color:        isActive ? op.color : "#94a3b8",
    border:       `1px solid ${isActive ? op.border : "#334155"}`,
    borderRadius: 5,
    padding:      "5px 12px",
    fontSize:     11,
    fontWeight:   600,
    cursor:       isBusy ? "not-allowed" : "pointer",
    transition:   "background 0.15s, color 0.15s",
    minWidth:     42,
    textAlign:    "center",
  };
}

const hintStyle: React.CSSProperties = {
  fontSize:   10,
  color:      "#334155",
  marginTop:  10,
  lineHeight: 1.5,
};
