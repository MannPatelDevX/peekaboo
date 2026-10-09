import type { Decision, PendingView } from "../types";

type Permission = Extract<PendingView, { kind: "permission" }>;

function detail(input: Permission["input"]): string {
  if (!input) return "";
  for (const k of ["command", "file_path", "path", "url", "pattern"]) {
    if (typeof input[k] === "string") return input[k] as string;
  }
  return JSON.stringify(input).slice(0, 200);
}

export function ApprovalCard({ req, onDecide }: { req: Permission; onDecide: (d: Decision) => void }) {
  return (
    <div className="card approval">
      <span className="agent">{req.agent} · {req.project}</span>
      <div className="tool">{req.tool}</div>
      <code className="detail">{detail(req.input)}</code>
      <div className="actions">
        <button className="primary" onClick={() => onDecide({ type: "allow", always: false })}>Allow</button>
        <button onClick={() => onDecide({ type: "allow", always: true })}>Always</button>
        <button className="danger" onClick={() => onDecide({ type: "deny" })}>Deny</button>
        <button className="ghost-btn" onClick={() => onDecide({ type: "passthrough" })}>Terminal</button>
      </div>
    </div>
  );
}
