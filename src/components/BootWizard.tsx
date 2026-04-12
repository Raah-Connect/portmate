import { useState, useEffect } from "react";
import { invoke }  from "@tauri-apps/api/core";
import { listen }  from "@tauri-apps/api/event";
import { open }    from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { TerminalOutput } from "./TerminalOutput";

type Step = "welcome" | "setup" | "download" | "boot" | "existing" | "key";

interface PlatformInfo { os: string; arch: string; supported: boolean; }
interface Props { onComplete?: () => void; }

const NEW_SHIP_STEPS: Step[] = ["welcome", "setup", "download", "boot"];

export function BootWizard({ onComplete }: Props) {
  const [step, setStep]               = useState<Step>("welcome");
  const [platform, setPlatform]       = useState<PlatformInfo | null>(null);

  // New comet state
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

  // Existing ship state
  const [existingPierPath, setExistingPierPath]     = useState("");
  const [existingBooting, setExistingBooting]       = useState(false);
  const [existingShipUrl, setExistingShipUrl]       = useState("");
  const [existingAccessCode, setExistingAccessCode] = useState("");
  const [existingLogs, setExistingLogs]             = useState<string[]>([]);

  // Key file boot state
  const [keyFilePath, setKeyFilePath]   = useState("");
  const [keyShipName, setKeyShipName]   = useState("");
  const [keyPierDir, setKeyPierDir]     = useState("");
  const [keyBooting, setKeyBooting]     = useState(false);
  const [keyShipUrl, setKeyShipUrl]     = useState("");
  const [keyAccessCode, setKeyAccessCode] = useState("");
  const [keyLogs, setKeyLogs]           = useState<string[]>([]);

  useEffect(() => {
    invoke<PlatformInfo>("get_platform_info").then(setPlatform);

    const cleanups = [
      listen<{ percent: number }>("download-progress", e =>
        setProgress(Math.round(e.payload.percent))
      ),
      listen<{ line: string; pier_path?: string }>("ship-log", e => {
        if (existingPierPath && e.payload.pier_path === existingPierPath) {
          setExistingLogs(prev => [...prev.slice(-300), e.payload.line]);
        } else if (keyPierDir && e.payload.pier_path?.startsWith(keyPierDir)) {
          setKeyLogs(prev => [...prev.slice(-300), e.payload.line]);
        } else {
          setLogs(prev => [...prev.slice(-300), e.payload.line]);
        }
      }),
      listen<{ url: string; pier_path?: string }>("ship-ready", e => {
        if (existingPierPath && e.payload.pier_path === existingPierPath) {
          setExistingShipUrl(e.payload.url);
        } else if (keyPierDir && e.payload.pier_path?.startsWith(keyPierDir)) {
          setKeyShipUrl(e.payload.url);
        } else {
          setShipUrl(e.payload.url);
        }
      }),
      listen<{ code: string; pier_path?: string }>("ship-code", e => {
        if (existingPierPath && e.payload.pier_path === existingPierPath) {
          setExistingAccessCode(e.payload.code);
        } else if (keyPierDir && e.payload.pier_path?.startsWith(keyPierDir)) {
          setKeyAccessCode(e.payload.code);
        } else {
          setAccessCode(e.payload.code);
        }
      }),
      listen("ship-exited", () =>
        setLogs(prev => [...prev, "[portmate] Ship process ended."])
      ),
    ];

    return () => { cleanups.forEach(p => p.then(fn => fn())); };
  }, [existingPierPath, keyPierDir]);

  // ── New comet handlers ─────────────────────────────────────────────────────

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

  // ── Existing ship handlers ─────────────────────────────────────────────────

  async function pickExistingPier() {
    const selected = await open({
      directory: true,
      title: "Select your existing Urbit pier folder",
    });
    if (typeof selected === "string") setExistingPierPath(selected);
  }

  async function handleBootExisting() {
    setError(""); setExistingBooting(true);
    try {
      await invoke("boot_existing", { pierPath: existingPierPath });
    } catch (e) {
      setError(String(e)); setExistingBooting(false);
    }
  }

  // ── Key file handlers ──────────────────────────────────────────────────────

  async function pickKeyFile() {
    const selected = await open({
      title: "Select your .key file",
      filters: [{ name: "Key file", extensions: ["key"] }],
    });
    if (typeof selected === "string") {
      setKeyFilePath(selected);
      // Derive ship name from filename e.g. worteg-rovder-fidzod-fidfes.key
      const filename = selected.split(/[\\/]/).pop() ?? "";
      const name = filename.replace(/\.key$/, "").replace(/^~/, "");
      setKeyShipName(name);
    }
  }

  async function pickKeyPierDir() {
    const selected = await open({
      directory: true,
      title: "Choose where to store the new pier",
    });
    if (typeof selected === "string") setKeyPierDir(selected);
  }

  async function handleBootKey() {
    setError(""); setKeyBooting(true);
    try {
      await invoke("boot_key", { keyFilePath, pierDir: keyPierDir });
    } catch (e) {
      setError(String(e)); setKeyBooting(false);
    }
  }

  const stepIndex = NEW_SHIP_STEPS.indexOf(step);

  return (
    <div style={{ maxWidth: 680, margin: "0 auto", padding: "32px 24px", fontFamily: "system-ui, sans-serif" }}>

      {/* Progress dots — only shown for new ship flow */}
      {step !== "existing" && step !== "key" && (
        <div style={{ display: "flex", gap: 8, marginBottom: 32 }}>
          {NEW_SHIP_STEPS.map((s, i) => (
            <div key={s} style={{
              width: 10, height: 10, borderRadius: "50%",
              background: i <= stepIndex ? "#0070f3" : "#ddd"
            }} />
          ))}
        </div>
      )}

      {/* ── Welcome ── */}
      {step === "welcome" && (
        <div>
          <h2>Welcome to Portmate</h2>
          <p>Let's get your Urbit ship running. Choose an option below:</p>

          {platform && (
            <p style={{ color: "#666", fontSize: 14 }}>
              Detected: <strong>{platform.os}</strong> / <strong>{platform.arch}</strong>
              {!platform.supported && <span style={{ color: "red" }}> — unsupported platform</span>}
            </p>
          )}

          <div style={{ display: "flex", flexDirection: "column", gap: 12, maxWidth: 320 }}>
            <button
              onClick={() => setStep("setup")}
              disabled={!platform?.supported}
              style={btnStyle}
            >
              🌑 Boot New Comet
            </button>
            <button
              onClick={() => { setError(""); setStep("key"); }}
              style={btnStyle}
            >
              🔑 Boot from Key File
            </button>
            <button
              onClick={() => { setError(""); setStep("existing"); }}
              style={btnSecStyle}
            >
              📂 Boot Existing Ship
            </button>
          </div>
        </div>
      )}

      {/* ── Boot from Key File ── */}
      {step === "key" && (
        <div>
          <h2>Boot from Key File</h2>
          <p style={{ color: "#666", fontSize: 14 }}>
            Use this to boot a moon, planet, star, or galaxy for the first time
            using a <code>.key</code> file. The ship name will be read from the filename.
          </p>

          <label style={labelStyle}>Key file (.key)</label>
          <div style={{ display: "flex", gap: 8, marginBottom: 8 }}>
            <input
              readOnly
              value={keyFilePath}
              placeholder="Select your .key file…"
              style={{ ...inputStyle, flex: 1, cursor: "pointer" }}
              onClick={pickKeyFile}
            />
            <button onClick={pickKeyFile} style={btnSmallStyle}>Browse</button>
          </div>
          {keyShipName && (
            <p style={{ fontSize: 13, color: "#555", marginBottom: 16 }}>
              Ship name detected: <strong>~{keyShipName}</strong>
            </p>
          )}

          <label style={labelStyle}>Where should the pier be created?</label>
          <div style={{ display: "flex", gap: 8, marginBottom: 20 }}>
            <input
              readOnly
              value={keyPierDir}
              placeholder="Choose a directory…"
              style={{ ...inputStyle, flex: 1, cursor: "pointer" }}
              onClick={pickKeyPierDir}
            />
            <button onClick={pickKeyPierDir} style={btnSmallStyle}>Browse</button>
          </div>
          {keyPierDir && keyShipName && (
            <p style={{ fontSize: 13, color: "#888", marginBottom: 16 }}>
              Pier will be created at: <code>{keyPierDir}/{keyShipName}</code>
            </p>
          )}

          {error && <p style={{ color: "red", fontSize: 14 }}>{error}</p>}

          {!keyBooting && !keyShipUrl && (
            <div style={{ display: "flex", gap: 12 }}>
              <button onClick={() => { setStep("welcome"); setError(""); }} style={btnSecStyle}>
                ← Back
              </button>
              <button
                onClick={handleBootKey}
                disabled={!keyFilePath || !keyPierDir}
                style={btnStyle}
              >
                Boot Ship 🚀
              </button>
            </div>
          )}

          {keyBooting && !keyShipUrl && (
            <p style={{ color: "#666" }}>⏳ Booting… this can take a few minutes.</p>
          )}

          {keyShipUrl && (
            <div style={{
              background: "#f0fdf4", border: "1px solid #86efac",
              borderRadius: 8, padding: "16px", marginBottom: 16
            }}>
              <p style={{ margin: 0, fontWeight: 600, color: "#166534" }}>✅ Ship is running!</p>
              {keyAccessCode ? (
                <p style={{ marginTop: 8, fontFamily: "monospace", fontSize: 16 }}>
                  Access code: <strong>{keyAccessCode}</strong>
                </p>
              ) : (
                <p style={{ fontSize: 13, color: "#555", marginTop: 8 }}>
                  Waiting for access code from dojo…
                </p>
              )}
              <button
                onClick={() => { openUrl(keyShipUrl); onComplete?.(); }}
                style={{ ...btnStyle, marginTop: 12 }}
              >
                Open Landscape →
              </button>
            </div>
          )}

          {keyLogs.length > 0 && <TerminalOutput logs={keyLogs} />}
        </div>
      )}

      {/* ── Existing Ship ── */}
      {step === "existing" && (
        <div>
          <h2>Boot Existing Ship</h2>
          <p style={{ color: "#666", fontSize: 14 }}>
            Select the pier folder of your existing Urbit ship (moon, planet, comet, etc).
            The Urbit binary will be detected automatically from the same directory — or
            downloaded if not found.
          </p>

          <label style={labelStyle}>Pier folder</label>
          <div style={{ display: "flex", gap: 8, marginBottom: 20 }}>
            <input
              readOnly
              value={existingPierPath}
              placeholder="Select your pier folder…"
              style={{ ...inputStyle, flex: 1, cursor: "pointer" }}
              onClick={pickExistingPier}
            />
            <button onClick={pickExistingPier} style={btnSmallStyle}>Browse</button>
          </div>

          {error && <p style={{ color: "red", fontSize: 14 }}>{error}</p>}

          {!existingBooting && !existingShipUrl && (
            <div style={{ display: "flex", gap: 12 }}>
              <button onClick={() => { setStep("welcome"); setError(""); }} style={btnSecStyle}>
                ← Back
              </button>
              <button
                onClick={handleBootExisting}
                disabled={!existingPierPath}
                style={btnStyle}
              >
                Boot Ship 🚀
              </button>
            </div>
          )}

          {existingBooting && !existingShipUrl && (
            <p style={{ color: "#666" }}>⏳ Booting… this can take a few minutes.</p>
          )}

          {existingShipUrl && (
            <div style={{
              background: "#f0fdf4", border: "1px solid #86efac",
              borderRadius: 8, padding: "16px", marginBottom: 16
            }}>
              <p style={{ margin: 0, fontWeight: 600, color: "#166534" }}>✅ Ship is running!</p>
              {existingAccessCode ? (
                <p style={{ marginTop: 8, fontFamily: "monospace", fontSize: 16 }}>
                  Access code: <strong>{existingAccessCode}</strong>
                </p>
              ) : (
                <p style={{ fontSize: 13, color: "#555", marginTop: 8 }}>
                  Waiting for access code from dojo…
                </p>
              )}
              <button
                onClick={() => { openUrl(existingShipUrl); onComplete?.(); }}
                style={{ ...btnStyle, marginTop: 12 }}
              >
                Open Landscape →
              </button>
            </div>
          )}

          {existingLogs.length > 0 && <TerminalOutput logs={existingLogs} />}
        </div>
      )}

      {/* ── Setup (new comet) ── */}
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

      {/* ── Boot (new comet) ── */}
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
                onClick={() => { openUrl(shipUrl); onComplete?.(); }}
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
