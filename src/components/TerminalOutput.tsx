import { useEffect, useRef } from "react";

interface Props {
  logs: string[];
}

export function TerminalOutput({ logs }: Props) {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  return (
    <div style={{
      background: "#0d1117",
      color: "#58a6ff",
      fontFamily: "monospace",
      fontSize: "12px",
      padding: "12px",
      borderRadius: "6px",
      height: "280px",
      overflowY: "auto",
      whiteSpace: "pre-wrap",
      wordBreak: "break-all",
    }}>
      {logs.map((line, i) => (
        <div key={i} style={{ color: line.startsWith("[portmate]") ? "#3fb950" : "#58a6ff" }}>
          {line}
        </div>
      ))}
      <div ref={bottomRef} />
    </div>
  );
}