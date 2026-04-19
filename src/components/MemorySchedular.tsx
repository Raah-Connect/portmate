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
	lastRunAt?: number | null;
	nextRunAt?: number | null;
	lastStatus?: string | null;
	lastError?: string | null;
	running?: boolean;
}

interface MemoryScheduleUpdatedPayload {
	pierPath: string;
}

interface ScheduleRowState {
	intervalDays: number;
	enabled: boolean;
	hasSchedule: boolean;
	saving: boolean;
	deleting: boolean;
	lastRunAt: number | null;
	nextRunAt: number | null;
	lastStatus: string | null;
	lastError: string;
	running: boolean;
	error: string;
}

type ScheduleRowMap = Record<OpId, ScheduleRowState>;

interface Props {
	ship: ShipInfo;
}

const OPS: { id: OpId; label: string; desc: string }[] = [
	{ id: "pack", label: "Pack", desc: "Compact loom memory" },
	{ id: "meld", label: "Meld", desc: "Deduplicate loom data" },
	{ id: "roll", label: "Roll", desc: "Compact event log" },
	{ id: "chop", label: "Chop", desc: "Trim old snapshots" },
];

function createDefaultRowState(): ScheduleRowState {
	return {
		intervalDays: 7,
		enabled: true,
		hasSchedule: false,
		saving: false,
		deleting: false,
		lastRunAt: null,
		nextRunAt: null,
		lastStatus: null,
		lastError: "",
		running: false,
		error: "",
	};
}

function createInitialRows(): ScheduleRowMap {
	return {
		pack: createDefaultRowState(),
		meld: createDefaultRowState(),
		roll: createDefaultRowState(),
		chop: createDefaultRowState(),
	};
}

export function MemorySchedular({ ship }: Props) {
	const [rows, setRows] = useState<ScheduleRowMap>(() => createInitialRows());
	const [loading, setLoading] = useState(true);
	const [message, setMessage] = useState("");
	const [error, setError] = useState("");

	async function loadSchedules() {
		setLoading(true);
		setError("");

		try {
			const schedules = await invoke<MemorySchedule[]>("get_memory_schedules_for_ship", {
				pierPath: ship.pierPath,
			});
			const nextRows = createInitialRows();

			for (const schedule of schedules) {
				nextRows[schedule.op] = {
					...createDefaultRowState(),
					intervalDays: schedule.intervalDays,
					enabled: schedule.enabled,
					hasSchedule: true,
					lastRunAt: schedule.lastRunAt ?? null,
					nextRunAt: schedule.nextRunAt ?? null,
					lastStatus: schedule.lastStatus ?? null,
					lastError: schedule.lastError ?? "",
					running: schedule.running ?? false,
					error: schedule.lastStatus === "error" ? schedule.lastError ?? "" : "",
				};
			}

			setRows(nextRows);
			setMessage(schedules.length === 0 ? "No schedules set for this ship." : "");
		} catch (e) {
			setError(String(e));
			setMessage("");
		} finally {
			setLoading(false);
		}
	}

	useEffect(() => {
		void loadSchedules();
	}, [ship.pierPath]);

	useEffect(() => {
		let unlisten: (() => void) | undefined;

		listen<MemoryScheduleUpdatedPayload>("memory-schedule-updated", (event) => {
			if (event.payload.pierPath !== ship.pierPath) return;
			void loadSchedules();
		}).then((fn) => {
			unlisten = fn;
		});

		return () => {
			unlisten?.();
		};
	}, [ship.pierPath]);

	function updateRow(op: OpId, updater: (current: ScheduleRowState) => ScheduleRowState) {
		setRows((currentRows) => ({
			...currentRows,
			[op]: updater(currentRows[op]),
		}));
	}

	function setIntervalForOp(op: OpId, intervalDays: number) {
		updateRow(op, (current) => ({
			...current,
			intervalDays,
			error: "",
		}));
	}

	function setEnabledForOp(op: OpId, enabled: boolean) {
		updateRow(op, (current) => ({
			...current,
			enabled,
			error: "",
		}));
	}

	async function saveSchedule(op: OpId) {
		const row = rows[op];
		const safeInterval = Math.max(1, Math.floor(Number(row.intervalDays) || 1));

		setError("");
		setMessage("");
		updateRow(op, (current) => ({
			...current,
			intervalDays: safeInterval,
			saving: true,
			lastError: "",
			error: "",
		}));

		try {
			await invoke("set_memory_schedule", {
				schedule: {
					pierPath: ship.pierPath,
					op,
					intervalDays: safeInterval,
					enabled: row.enabled,
				},
			});

			setMessage(`${OPS.find((entry) => entry.id === op)?.label ?? op} schedule saved.`);
			await loadSchedules();
		} catch (e) {
			updateRow(op, (current) => ({
				...current,
				saving: false,
				lastStatus: "error",
				lastError: String(e),
				error: String(e),
			}));
		} finally {
			updateRow(op, (current) => ({
				...current,
				saving: false,
			}));
		}
	}

	async function clearSchedule(op: OpId) {
		setError("");
		setMessage("");
		updateRow(op, (current) => ({
			...current,
			deleting: true,
			lastError: "",
			error: "",
		}));

		try {
			await invoke("clear_memory_schedule", { pierPath: ship.pierPath, op });
			setMessage(`${OPS.find((entry) => entry.id === op)?.label ?? op} schedule removed.`);
			await loadSchedules();
		} catch (e) {
			updateRow(op, (current) => ({
				...current,
				deleting: false,
				lastStatus: "error",
				lastError: String(e),
				error: String(e),
			}));
		} finally {
			updateRow(op, (current) => ({
				...current,
				deleting: false,
			}));
		}
	}

	async function clearAllSchedules() {
		setLoading(true);
		setError("");
		setMessage("");

		try {
			await invoke("clear_all_memory_schedules_for_ship", { pierPath: ship.pierPath });
			setMessage("All schedules removed for this ship.");
			await loadSchedules();
		} catch (e) {
			setError(String(e));
			setLoading(false);
		}
	}

	const hasAnySchedule = OPS.some((entry) => rows[entry.id].hasSchedule);

	return (
		<div style={wrapStyle}>
			<div style={sectionLabelStyle}>Memory Scheduler</div>
			<div style={headerRowStyle}>
				<div>
					<div style={titleStyle}>Maintenance by operation</div>
					<div style={subtitleStyle}>{ship.name} - {ship.pierPath}</div>
				</div>
				<button
					onClick={() => void clearAllSchedules()}
					disabled={loading || !hasAnySchedule}
					style={dangerBtnStyle(loading || !hasAnySchedule)}
				>
					Clear all
				</button>
			</div>

			{loading && <div style={bannerStyle("loading")}>Loading schedules...</div>}
			{!loading && message && !error && <div style={bannerStyle("ok")}>{message}</div>}
			{!loading && error && <div style={bannerStyle("error")}>{error}</div>}

			<div style={tableWrapStyle}>
				<table style={tableStyle}>
					<thead>
						<tr>
							<th style={thStyle}>Operation</th>
							<th style={thStyle}>Every N days</th>
							<th style={thStyle}>Enabled</th>
							<th style={thStyle}>Last run</th>
							<th style={thStyle}>Next run</th>
							<th style={thStyle}>Status</th>
							<th style={thStyle}>Actions</th>
						</tr>
					</thead>
					<tbody>
						{OPS.map((entry) => {
							const row = rows[entry.id];
							const busy = loading || row.saving || row.deleting;

							return (
								<tr key={entry.id} style={trStyle}>
									<td style={tdStyle}>
										<div style={opLabelStyle}>{entry.label}</div>
										<div style={helperTextStyle}>{entry.desc}</div>
									</td>
									<td style={tdStyle}>
										<div style={intervalCellStyle}>
											<input
												type="number"
												min={1}
												step={1}
												value={row.intervalDays}
												onChange={(e) => setIntervalForOp(entry.id, Number(e.target.value))}
												disabled={busy}
												style={inputStyle}
											/>
											<div style={presetGroupStyle}>
												<button
													onClick={() => setIntervalForOp(entry.id, 1)}
													disabled={busy}
													style={presetBtnStyle(row.intervalDays === 1)}
												>
													1d
												</button>
												<button
													onClick={() => setIntervalForOp(entry.id, 7)}
													disabled={busy}
													style={presetBtnStyle(row.intervalDays === 7)}
												>
													7d
												</button>
											</div>
										</div>
									</td>
									<td style={tdStyle}>
										<label style={toggleStyle}>
											<input
												type="checkbox"
												checked={row.enabled}
												disabled={busy}
												onChange={(e) => setEnabledForOp(entry.id, e.target.checked)}
											/>
											<span>{row.enabled ? "Active" : "Paused"}</span>
										</label>
									</td>
									<td style={tdStyle}>
										<div style={timeValueStyle}>{formatScheduleTime(row.lastRunAt)}</div>
									</td>
									<td style={tdStyle}>
										<div style={timeValueStyle}>{formatScheduleTime(row.nextRunAt)}</div>
									</td>
									<td style={tdStyle}>
										<span style={statusPillStyle(row.running, row.lastStatus, row.hasSchedule)}>
											{formatStatusLabel(row)}
										</span>
										{row.lastError && !row.error && <div style={rowErrorStyle}>{row.lastError}</div>}
										{row.error && <div style={rowErrorStyle}>{row.error}</div>}
									</td>
									<td style={tdStyle}>
										<div style={actionRowStyle}>
											<button
												onClick={() => void saveSchedule(entry.id)}
												disabled={busy}
												style={primaryBtnStyle}
											>
												{row.saving ? "Saving..." : row.hasSchedule ? "Update" : "Save"}
											</button>
											<button
												onClick={() => void clearSchedule(entry.id)}
												disabled={busy || !row.hasSchedule}
												style={dangerBtnStyle(busy || !row.hasSchedule)}
											>
												{row.deleting ? "Removing..." : "Clear"}
											</button>
										</div>
									</td>
								</tr>
							);
						})}
					</tbody>
				</table>
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
	marginBottom: 8,
};

const headerRowStyle: React.CSSProperties = {
	display: "flex",
	justifyContent: "space-between",
	alignItems: "flex-start",
	gap: 12,
	marginBottom: 10,
	flexWrap: "wrap",
};

const titleStyle: React.CSSProperties = {
	fontSize: 15,
	fontWeight: 700,
	color: "#e2e8f0",
};

const subtitleStyle: React.CSSProperties = {
	fontSize: 11,
	color: "#64748b",
	marginTop: 2,
	wordBreak: "break-all",
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

const tableWrapStyle: React.CSSProperties = {
	overflowX: "auto",
	borderRadius: 10,
	border: "1px solid #1e293b",
	background: "#08111f",
};

const tableStyle: React.CSSProperties = {
	width: "100%",
	borderCollapse: "collapse",
	minWidth: 980,
};

const thStyle: React.CSSProperties = {
	textAlign: "left",
	fontSize: 11,
	fontWeight: 600,
	letterSpacing: "0.04em",
	textTransform: "uppercase",
	color: "#64748b",
	padding: "12px 14px",
	borderBottom: "1px solid #1e293b",
	background: "#0f172a",
};

const helperTextStyle: React.CSSProperties = {
	fontSize: 10,
	color: "#475569",
};

const trStyle: React.CSSProperties = {
	borderBottom: "1px solid #1e293b",
};

const tdStyle: React.CSSProperties = {
	padding: "14px",
	verticalAlign: "top",
};

const opLabelStyle: React.CSSProperties = {
	fontSize: 13,
	fontWeight: 700,
	color: "#e2e8f0",
	marginBottom: 4,
};

const timeValueStyle: React.CSSProperties = {
	fontSize: 11,
	color: "#cbd5e1",
	whiteSpace: "nowrap",
};

const intervalCellStyle: React.CSSProperties = {
	display: "flex",
	flexDirection: "column",
	gap: 8,
	maxWidth: 140,
};

const presetGroupStyle: React.CSSProperties = {
	display: "flex",
	gap: 6,
};

const inputStyle: React.CSSProperties = {
	borderRadius: 6,
	border: "1px solid #334155",
	background: "#0b1220",
	color: "#e2e8f0",
	padding: "7px 8px",
	fontSize: 12,
	width: "100%",
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

function statusPillStyle(running: boolean, lastStatus: string | null, hasSchedule: boolean): React.CSSProperties {
	const tone = running
		? { border: "#1d4ed8", background: "#0b2545", color: "#bfdbfe" }
		: lastStatus === "success"
			? { border: "#14532d", background: "#052e1a", color: "#86efac" }
			: lastStatus === "error"
				? { border: "#7f1d1d", background: "#2b0b0b", color: "#fecaca" }
				: hasSchedule
					? { border: "#334155", background: "#111827", color: "#cbd5e1" }
					: { border: "#334155", background: "#111827", color: "#94a3b8" };

	return {
		display: "inline-flex",
		alignItems: "center",
		borderRadius: 999,
		padding: "4px 10px",
		fontSize: 11,
		fontWeight: 700,
		border: `1px solid ${tone.border}`,
		background: tone.background,
		color: tone.color,
	};
}

const rowErrorStyle: React.CSSProperties = {
	marginTop: 8,
	fontSize: 11,
	color: "#fca5a5",
	maxWidth: 220,
	wordBreak: "break-word",
};

function formatScheduleTime(timestamp: number | null): string {
	if (!timestamp) {
		return "--";
	}

	return new Intl.DateTimeFormat(undefined, {
		month: "short",
		day: "numeric",
		hour: "numeric",
		minute: "2-digit",
	}).format(new Date(timestamp * 1000));
}

function formatStatusLabel(row: ScheduleRowState): string {
	if (!row.hasSchedule) {
		return "Not set";
	}

	if (row.running) {
		return "Running";
	}

	if (row.lastStatus === "success") {
		return "Healthy";
	}

	if (row.lastStatus === "error") {
		return "Error";
	}

	return "Saved";
}

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
