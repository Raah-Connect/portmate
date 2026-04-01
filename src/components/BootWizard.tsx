import { useState, useEffect } from "react";
import { invoke }  from "@tauri-apps/api/core";
import { listen }  from "@tauri-apps/api/event";
import { open }    from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { TerminalOutput } from "./TerminalOutput";

type Step = "welcome" | "setup" | "download" | "boot";

interface PlatformInfo { os: string; arch: string; supported: boolean; }

const STEPS: Step[] = ["welcome", "setup", "download", "boot"];

export function BootWizard() {
  const [step, setStep]               = useState<Step>("welcome");
  const [platform, setPlatform]       = useState<PlatformInfo | null>(null);
  const [pierDir, setPierDir]         = useState("");
  const [cometName, setCometName]     = useState("my-urbit");
  const [binaryPath, setBinaryPath]   = useState("");
  const [progress, setProgress]       = useState(0);
  const [downloading, setDownloading] = useState(false);
  const [booting, setBooting]         = useState(false);
  const [logs, setLogs]               = useState<string[]>([]);
  const [shipUrl, setShipUrl]         = useState("");
  const [accessCode, setAccessCode]   = useState("");
  const [error, setError]             = useState("");

  useEffect(() => {
    invoke<PlatformInfo>("get_platform_info").then(setPlatform);

    const cleanups = [
      listen<{ percent: number }>("download-progress", e =>
        setProgress(Math.round(e.payload.percent))
      ),
      listen<{ line: string }>("ship-log", e =>
        setLogs(prev => [...prev.slice(-300), e.payload.line])
      ),
      listen<{ url: string }>("ship-ready", e =>
        setShipUrl(e.payload.url)
      ),
      listen<{ code: string }>("ship-code", e =>
        setAccessCode(e.payload.code)
      ),
      listen("ship-exited", () =>
        setLogs(prev => [...prev, "[portmate] Ship process ended."])
      ),
    ];

    return () => { cleanups.forEach(p => p.then(fn => fn())); };
  }, []);

  async function pickDirectory() {
    const selected = await open({ directory: true, title: "Choose where to store your ship" });
    if (typeof selected === "string") setPierDir(selected);
  }

  async function handleDownload() {
    setError(""); setDownloading(true);
    try {
      const path = await invoke<string>("download_urbit", { destDir: pierDir });
      setBinaryPath(path);
      setStep("boot");
    } catch (e) {
      setError(String(e));
    } finally {
      setDownloading(false);
    }
  }

  async function handleBoot() {
    setError(""); setBooting(true);
    try {
      await invoke("boot_comet", { binaryPath, pierDir, cometName });
    } catch (e) {
      setError(String(e)); setBooting(false);
    }
  }
  async function handleSelectExisting() {
    const selected = await open({
      title: "Select Urbit binary",
      filters: [{ name: "urbit", extensions: ["*"] }],
    });
    if (typeof selected === "string") {
      setBinaryPath(selected);
      setStep("boot");
    }
  }
  const stepIndex = STEPS.indexOf(step);

  return (
    <div style={{ maxWidth: 680, margin: "0 auto", padding: "32px 24px", fontFamily: "system-ui, sans-serif" }}>
      {/* Progress dots */}
      <div style={{ display: "flex", gap: 8, marginBottom: 32 }}>
        {STEPS.map((s, i) => (
          <div key={s} style={{
            width: 10, height: 10, borderRadius: "50%",
            background: i <= stepIndex ? "#0070f3" : "#ddd"
          }} />
        ))}
      </div>

      {/* ── Welcome ── */}
      {step === "welcome" && (
        <div>
          <h2>Welcome to Portmate</h2>
          <p>Let's get your Urbit ship running. This wizard will:</p>
          <ol>
            <li>Download the Urbit runtime for your machine</li>
            <li>Boot a comet (a free, temporary identity)</li>
            <li>Open your ship's web interface</li>
          </ol>

          {platform && (
            <p style={{ color: "#666", fontSize: 14 }}>
              Detected: <strong>{platform.os}</strong> / <strong>{platform.arch}</strong>
              {!platform.supported && <span style={{ color: "red" }}> — unsupported platform</span>}
            </p>
          )}

          <button
            onClick={() => setStep("setup")}
            disabled={!platform?.supported}
            style={btnStyle}
          >
            Get Started →
          </button>
        </div>
      )}

      {/* ── Setup ── */}
      {step === "setup" && (
        <div>
          <h2>Set Up Your Ship</h2>

          <label style={labelStyle}>Where should your ship live?</label>
          <div style={{ display: "flex", gap: 8, marginBottom: 20 }}>
            <input
              readOnly value={pierDir}
              placeholder="Choose a directory…"
              style={{ ...inputStyle, flex: 1, cursor: "pointer" }}
              onClick={pickDirectory}
            />
            <button onClick={pickDirectory} style={btnSmallStyle}>Browse</button>
          </div>

          <label style={labelStyle}>Name your comet's folder</label>
          <input
            value={cometName}
            onChange={e => setCometName(e.target.value)}
            placeholder="my-urbit"
            style={{ ...inputStyle, marginBottom: 24 }}
          />
          <p style={{ fontSize: 13, color: "#888", marginTop: -16, marginBottom: 24 }}>
            This is just the folder name — Urbit will assign your actual comet identity automatically.
          </p>

          <div style={{ display: "flex", gap: 12 }}>
            <button onClick={() => setStep("welcome")} style={btnSecStyle}>← Back</button>
            <button
              onClick={() => setStep("download")}
              disabled={!pierDir || !cometName.trim()}
              style={btnStyle}
            >
              Next →
            </button>
          </div>
        </div>
      )}

      {/* ── Download ── */}
      {step === "download" && (
  <div>
    <h2>Downloading Urbit Runtime</h2>
    <p style={{ color: "#666", fontSize: 14 }}>
      Fetching the Urbit runtime for {platform?.os}/{platform?.arch}…
    </p>

    {downloading ? (
      <div>
        <div style={{ background: "#eee", borderRadius: 4, height: 12, marginBottom: 8 }}>
          <div style={{
            background: "#0070f3", height: "100%",
            borderRadius: 4, width: `${progress}%`,
            transition: "width 0.2s ease"
          }} />
        </div>
        <p style={{ fontSize: 13, color: "#666" }}>{progress}% downloaded</p>
      </div>
    ) : (
      <>
        {error && <p style={{ color: "red", fontSize: 14 }}>{error}</p>}
        <div style={{ display: "flex", gap: 12 }}>
          <button onClick={() => setStep("setup")} style={btnSecStyle}>← Back</button>
          <button onClick={handleDownload} style={btnStyle}>Download Runtime</button>
        </div>

        <div style={{ marginTop: 24, paddingTop: 24, borderTop: "1px solid #eee" }}>
        <p style={{ fontSize: 14, color: "#666", marginBottom: 12 }}>
          Already have the Urbit runtime?
        </p>
        <button onClick={handleSelectExisting} style={btnSecStyle}>
          Browse for existing binary
        </button>
      </div>
      </>
    )}
  </div>
)}

      {/* ── Boot ── */}
      {step === "boot" && (
        <div>
          <h2>Boot Your Ship</h2>

          {!booting && !shipUrl && (
            <>
              <p>Ready to boot <strong>{cometName}</strong>.</p>
              <p style={{ fontSize: 13, color: "#666" }}>
                Mining your comet identity takes a few minutes — the terminal below will show progress.
              </p>
              {error && <p style={{ color: "red", fontSize: 14 }}>{error}</p>}
              <button onClick={handleBoot} style={btnStyle}>Boot Ship 🚀</button>
            </>
          )}

          {booting && !shipUrl && (
            <p style={{ color: "#666" }}>⏳ Booting… this can take a few minutes.</p>
          )}

          {shipUrl && (
            <div style={{
              background: "#f0fdf4", border: "1px solid #86efac",
              borderRadius: 8, padding: "16px", marginBottom: 16
            }}>
              <p style={{ margin: 0, fontWeight: 600, color: "#166534" }}>✅ Ship is running!</p>
              {accessCode ? (
                <p style={{ marginTop: 8, fontFamily: "monospace", fontSize: 16 }}>
                  Access code: <strong>{accessCode}</strong>
                </p>
              ) : (
                <p style={{ fontSize: 13, color: "#555", marginTop: 8 }}>
                  Waiting for access code from dojo…
                </p>
              )}
              <button
                onClick={() => openUrl(shipUrl)}
                style={{ ...btnStyle, marginTop: 12 }}
              >
                Open Landscape →
              </button>
            </div>
          )}

          <TerminalOutput logs={logs} />
        </div>
      )}
    </div>
  );
}

// ── Styles ────────────────────────────────────────────────────────────────────

const btnStyle: React.CSSProperties = {
  background: "#0070f3", color: "#fff", border: "none",
  borderRadius: 6, padding: "10px 20px", cursor: "pointer",
  fontSize: 14, fontWeight: 600,
};

const btnSecStyle: React.CSSProperties = {
  ...btnStyle, background: "#f0f0f0", color: "#333",
};

const btnSmallStyle: React.CSSProperties = {
  ...btnStyle, padding: "8px 14px",
};

const inputStyle: React.CSSProperties = {
  border: "1px solid #ddd", borderRadius: 6,
  padding: "8px 12px", fontSize: 14, width: "100%",
  boxSizing: "border-box" as const,
};

const labelStyle: React.CSSProperties = {
  display: "block", fontWeight: 600,
  fontSize: 14, marginBottom: 6,
};