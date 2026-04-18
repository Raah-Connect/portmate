import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ShipInfo } from "./ShipCard";

type OpId = "pack" | "meld" | "roll" | "chop";

interface MemorySchedule {
	pierPath: string;
	op: OpId;
	intervalDays: number;
	enabled: boolean;
}

interface MemoryScheduleUpdatedPayload {
	pierPath: string;
}

interface Props {
	ship: ShipInfo;
}

const OPS: { id: OpId; label: string; desc: string }[] = [
	{ id: "pack", label: "Pack", desc: "Compact loom memory" },
	{ id: "meld", label: "Meld", desc: "Deduplicate loom data" },
	{ id: "roll", label: "Roll", desc: "Compact event log" },
	{ id: "chop", label: "Chop", desc: "Trim old snapshots" },
];

export function MemorySchedular({ ship }: Props) {
	const [op, setOp] = useState<OpId>("pack");
	const [intervalDays, setIntervalDays] = useState(7);
	const [enabled, setEnabled] = useState(true);
	const [hasSchedule, setHasSchedule] = useState(false);
	const [loading, setLoading] = useState(true);
	const [saving, setSaving] = useState(false);
	const [deleting, setDeleting] = useState(false);
	const [message, setMessage] = useState("");
	const [error, setError] = useState("");

	async function loadSchedule() {
		setLoading(true);
		setError("");

		try {
			const schedule = await invoke<MemorySchedule | null>("get_memory_schedule", {
				pierPath: ship.pierPath,
			});

			if (!schedule) {
				setHasSchedule(false);
				setMessage("No schedule set for this ship.");
				return;
			}

			setOp(schedule.op);
			setIntervalDays(schedule.intervalDays);
			setEnabled(schedule.enabled);
			setHasSchedule(true);
			setMessage("");
		} catch (e) {
			setError(String(e));
			setMessage("");
		} finally {
			setLoading(false);
		}
	}

	useEffect(() => {
		void loadSchedule();
	}, [ship.pierPath]);

	useEffect(() => {
		let unlisten: (() => void) | undefined;

		listen<MemoryScheduleUpdatedPayload>("memory-schedule-updated", (event) => {
			if (event.payload.pierPath !== ship.pierPath) return;
			void loadSchedule();
		}).then((fn) => {
			unlisten = fn;
		});

		return () => {
			unlisten?.();
		};
	}, [ship.pierPath]);

	async function saveSchedule() {
		const safeInterval = Math.max(1, Math.floor(Number(intervalDays) || 1));

		setSaving(true);
		setError("");
		setMessage("");

		try {
			await invoke("set_memory_schedule", {
				schedule: {
					pierPath: ship.pierPath,
					op,
					intervalDays: safeInterval,
					enabled,
				},
			});

			setIntervalDays(safeInterval);
			setHasSchedule(true);
			setMessage("Schedule saved.");
		} catch (e) {
			setError(String(e));
		} finally {
			setSaving(false);
		}
	}

	async function clearSchedule() {
		setDeleting(true);
		setError("");
		setMessage("");

		try {
			await invoke("clear_memory_schedule", { pierPath: ship.pierPath });
			setHasSchedule(false);
			setMessage("Schedule removed.");
		} catch (e) {
			setError(String(e));
		} finally {
			setDeleting(false);
		}
	}

	const busy = loading || saving || deleting;

	return (
		<div style={wrapStyle}>
			<div style={sectionLabelStyle}>Memory Scheduler</div>

			{loading && <div style={bannerStyle("loading")}>Loading schedule...</div>}
			{!loading && message && !error && <div style={bannerStyle("ok")}>{message}</div>}
			{!loading && error && <div style={bannerStyle("error")}>{error}</div>}

			<div style={gridStyle}>
				<label style={fieldStyle}>
					<span style={fieldLabelStyle}>Operation</span>
					<select
						value={op}
						onChange={(e) => setOp(e.target.value as OpId)}
						disabled={busy}
						style={selectStyle}
					>
						{OPS.map((entry) => (
							<option key={entry.id} value={entry.id}>
								{entry.label}
							</option>
						))}
					</select>
					<span style={helperTextStyle}>{OPS.find((entry) => entry.id === op)?.desc ?? ""}</span>
				</label>

				<label style={fieldStyle}>
					<span style={fieldLabelStyle}>How often (days)</span>
					<input
						type="number"
						min={1}
						step={1}
						value={intervalDays}
						onChange={(e) => setIntervalDays(Number(e.target.value))}
						disabled={busy}
						style={inputStyle}
					/>
					<span style={helperTextStyle}>Use 1 for daily or 7 for weekly.</span>
				</label>
			</div>

			<div style={quickChoiceStyle}>
				<button
					onClick={() => setIntervalDays(1)}
					disabled={busy}
					style={presetBtnStyle(intervalDays === 1)}
				>
					Daily
				</button>
				<button
					onClick={() => setIntervalDays(7)}
					disabled={busy}
					style={presetBtnStyle(intervalDays === 7)}
				>
					Weekly
				</button>
			</div>

			<label style={toggleStyle}>
				<input
					type="checkbox"
					checked={enabled}
					disabled={busy}
					onChange={(e) => setEnabled(e.target.checked)}
				/>
				Enable scheduler for this ship
			</label>

			<div style={actionRowStyle}>
				<button onClick={saveSchedule} disabled={busy} style={primaryBtnStyle}>
					{saving ? "Saving..." : hasSchedule ? "Update schedule" : "Create schedule"}
				</button>
				<button
					onClick={clearSchedule}
					disabled={busy || !hasSchedule}
					style={dangerBtnStyle(busy || !hasSchedule)}
				>
					{deleting ? "Removing..." : "Remove schedule"}
				</button>
			</div>
		</div>
	);
}

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

function bannerStyle(kind: "loading" | "ok" | "error"): React.CSSProperties {
	const map = {
		loading: { bg: "#111827", border: "#334155", color: "#93c5fd" },
		ok: { bg: "#052e1a", border: "#14532d", color: "#86efac" },
		error: { bg: "#2b0b0b", border: "#7f1d1d", color: "#fca5a5" },
	};

	return {
		borderRadius: 6,
		border: `1px solid ${map[kind].border}`,
		background: map[kind].bg,
		color: map[kind].color,
		fontSize: 12,
		padding: "7px 10px",
		marginBottom: 10,
	};
}

const gridStyle: React.CSSProperties = {
	display: "grid",
	gridTemplateColumns: "1fr 1fr",
	gap: 8,
};

const fieldStyle: React.CSSProperties = {
	display: "flex",
	flexDirection: "column",
	gap: 5,
};

const fieldLabelStyle: React.CSSProperties = {
	fontSize: 11,
	color: "#64748b",
};

const helperTextStyle: React.CSSProperties = {
	fontSize: 10,
	color: "#475569",
};

const inputStyle: React.CSSProperties = {
	borderRadius: 6,
	border: "1px solid #334155",
	background: "#0b1220",
	color: "#e2e8f0",
	padding: "7px 8px",
	fontSize: 12,
};

const selectStyle: React.CSSProperties = {
	...inputStyle,
	appearance: "none",
};

const quickChoiceStyle: React.CSSProperties = {
	display: "flex",
	gap: 8,
	marginTop: 8,
};

function presetBtnStyle(active: boolean): React.CSSProperties {
	return {
		borderRadius: 999,
		border: `1px solid ${active ? "#3b82f6" : "#334155"}`,
		background: active ? "#0b2545" : "#111827",
		color: active ? "#bfdbfe" : "#94a3b8",
		fontSize: 11,
		padding: "4px 10px",
		cursor: "pointer",
	};
}

const toggleStyle: React.CSSProperties = {
	marginTop: 10,
	display: "flex",
	alignItems: "center",
	gap: 8,
	fontSize: 12,
	color: "#cbd5e1",
};

const actionRowStyle: React.CSSProperties = {
	marginTop: 12,
	display: "flex",
	gap: 8,
};

const primaryBtnStyle: React.CSSProperties = {
	borderRadius: 6,
	border: "1px solid #1e3a8a",
	background: "#0f2a58",
	color: "#dbeafe",
	fontSize: 12,
	fontWeight: 600,
	padding: "7px 10px",
	cursor: "pointer",
};

function dangerBtnStyle(disabled: boolean): React.CSSProperties {
	return {
		borderRadius: 6,
		border: `1px solid ${disabled ? "#334155" : "#7f1d1d"}`,
		background: disabled ? "#111827" : "#2b0b0b",
		color: disabled ? "#475569" : "#fecaca",
		fontSize: 12,
		fontWeight: 600,
		padding: "7px 10px",
		cursor: disabled ? "not-allowed" : "pointer",
	};
}
