import { useEffect, useState } from "react";
import { MemorySchedular } from "./MemorySchedular";
import type { ShipInfo } from "./ShipCard";

interface Props {
	ships: ShipInfo[];
}

export function GlobalSchedulerView({ ships }: Props) {
	const [selectedPierPath, setSelectedPierPath] = useState(ships[0]?.pierPath ?? "");

	useEffect(() => {
		if (ships.length === 0) {
			setSelectedPierPath("");
			return;
		}

		if (!ships.some((ship) => ship.pierPath === selectedPierPath)) {
			setSelectedPierPath(ships[0].pierPath);
		}
	}, [selectedPierPath, ships]);

	if (ships.length === 0) {
		return (
			<section style={emptyWrapStyle}>
				<div style={emptyTitleStyle}>No ships available for scheduling</div>
				<div style={emptyTextStyle}>Boot a ship first, then configure its maintenance plan here.</div>
			</section>
		);
	}

	const selectedShip = ships.find((ship) => ship.pierPath === selectedPierPath) ?? ships[0];

	return (
		<section style={wrapStyle}>
			<div style={topBarStyle}>
				<div>
					<div style={eyebrowStyle}>Global Scheduler</div>
					<h2 style={titleStyle}>Plan maintenance without opening a ship card</h2>
					<p style={subtitleStyle}>
						Pick a ship, review the next scheduled run, and manage each operation independently.
					</p>
				</div>
				<label style={pickerStyle}>
					<span style={pickerLabelStyle}>Ship</span>
					<select
						value={selectedShip.pierPath}
						onChange={(event) => setSelectedPierPath(event.target.value)}
						style={selectStyle}
					>
						{ships.map((ship) => (
							<option key={ship.pierPath} value={ship.pierPath}>
								{ship.name} - {ship.pierPath}
							</option>
						))}
					</select>
				</label>
			</div>

			<div style={schedulerCardStyle}>
				<MemorySchedular ship={selectedShip} />
			</div>
		</section>
	);
}

const wrapStyle: React.CSSProperties = {
	display: "flex",
	flexDirection: "column",
	gap: 18,
	padding: "24px",
	maxWidth: 1280,
	margin: "0 auto",
	width: "100%",
};

const topBarStyle: React.CSSProperties = {
	display: "flex",
	justifyContent: "space-between",
	alignItems: "flex-end",
	gap: 16,
	flexWrap: "wrap",
};

const eyebrowStyle: React.CSSProperties = {
	fontSize: 11,
	fontWeight: 700,
	letterSpacing: "0.08em",
	textTransform: "uppercase",
	color: "#60a5fa",
	marginBottom: 8,
};

const titleStyle: React.CSSProperties = {
	fontSize: 22,
	fontWeight: 700,
	color: "#f8fafc",
	margin: 0,
};

const subtitleStyle: React.CSSProperties = {
	fontSize: 13,
	color: "#94a3b8",
	marginTop: 8,
	maxWidth: 640,
	lineHeight: 1.5,
};

const pickerStyle: React.CSSProperties = {
	display: "flex",
	flexDirection: "column",
	gap: 6,
	minWidth: 320,
	maxWidth: 520,
	flex: "1 1 320px",
};

const pickerLabelStyle: React.CSSProperties = {
	fontSize: 11,
	fontWeight: 700,
	textTransform: "uppercase",
	letterSpacing: "0.06em",
	color: "#64748b",
};

const selectStyle: React.CSSProperties = {
	borderRadius: 8,
	border: "1px solid #334155",
	background: "#0f172a",
	color: "#e2e8f0",
	padding: "10px 12px",
	fontSize: 13,
	fontFamily: "inherit",
};

const schedulerCardStyle: React.CSSProperties = {
	borderRadius: 14,
	border: "1px solid #1e293b",
	background: "#0b1220",
	overflow: "hidden",
	boxShadow: "0 18px 50px rgba(2, 6, 23, 0.35)",
};

const emptyWrapStyle: React.CSSProperties = {
	display: "flex",
	flexDirection: "column",
	alignItems: "center",
	justifyContent: "center",
	textAlign: "center",
	padding: "80px 24px",
	color: "#64748b",
	minHeight: "50vh",
};

const emptyTitleStyle: React.CSSProperties = {
	fontSize: 20,
	fontWeight: 700,
	color: "#cbd5e1",
	marginBottom: 8,
};

const emptyTextStyle: React.CSSProperties = {
	fontSize: 14,
	color: "#64748b",
};