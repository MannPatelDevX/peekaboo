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
      <div className="row">
        <div className="tool">{req.tool}</div>
        <span className="badge">{req.project}</span>
      </div>
      <code className="detail">{detail(req.input)}</code>
      <div className="actions">
        <button className="primary" onClick={() => onDecide({ type: "allow", always: false })}>Allow</button>
        <button
          title={req.tool === "Bash" ? `Always allow this exact command in ${req.project}` : `Always allow ${req.tool} in ${req.project}`}
          onClick={() => onDecide({ type: "allow", always: true })}
        >
          Always
        </button>
        <button className="danger" onClick={() => onDecide({ type: "deny" })}>Deny</button>
        <button className="ghost-btn" onClick={() => onDecide({ type: "passthrough" })}>Terminal</button>
      </div>
    </div>
  );
}
