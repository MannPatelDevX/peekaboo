import type { AlwaysRule, HookStatus, OpenIn } from "../types";

interface Props {
  hooks: HookStatus[];
  rules: AlwaysRule[];
  onInstall: (agent: string) => void;
  onUninstall: (agent: string) => void;
  onRemoveRule: (rule: AlwaysRule) => void;
  openIn?: OpenIn;
  itermInstalled?: boolean;
  onOpenIn?: (v: OpenIn) => void;
}

const projectOf = (cwd: string) => cwd.split(/[\\/]/).filter(Boolean).pop() ?? cwd;

export function SettingsPanel({ hooks, rules, onInstall, onUninstall, onRemoveRule, openIn, itermInstalled, onOpenIn }: Props) {
  return (
    <div className="settings">
      {openIn && onOpenIn && (
        <>
          <h3>Sessions</h3>
          <div className="row setting">
            <div>
              <label htmlFor="open-in"><strong>Open Claude sessions in</strong></label>
              <div className="meta">Messages from Peekaboo arrive as real prompts in these terminals.</div>
            </div>
            <select id="open-in" aria-label="Open Claude sessions in" value={openIn} onChange={(e) => onOpenIn(e.target.value as OpenIn)}>
              <option value="terminal">Terminal</option>
              <option value="iterm2" disabled={!itermInstalled}>iTerm2{itermInstalled ? "" : " (not installed)"}</option>
            </select>
          </div>
        </>
      )}
      <h3>Agents</h3>
      {hooks.map((h) => (
        <div key={h.agent} className="row setting">
          <div>
            <strong>{h.agent}</strong>
            <div className="meta">{h.error ?? (h.stale ? "Hook program is missing. Repair to fix." : h.configPath)}</div>
          </div>
          {h.installed && h.stale ? (
            <button className="primary" onClick={() => onInstall(h.agent)}>Repair</button>
          ) : h.installed ? (
            <button onClick={() => onUninstall(h.agent)}>Uninstall</button>
          ) : (
            <button className="primary" disabled={!!h.error} onClick={() => onInstall(h.agent)}>Install</button>
          )}
        </div>
      ))}
      <h3>Always allowed</h3>
      {rules.length === 0 && <p className="empty">Nothing yet. Choose "Always" on a request to add one.</p>}
      {rules.map((r) => (
        <div key={`${r.agent}:${r.tool}:${r.cwd}:${r.command}`} className="row setting">
          <div>
            <strong>{r.tool}</strong>
            {r.command && <code className="rule-cmd"> {r.command}</code>}
            <div className="meta">{r.agent} · {r.cwd ? projectOf(r.cwd) : "any project"}</div>
          </div>
          <button aria-label={`Remove ${r.tool}`} onClick={() => onRemoveRule(r)}>✕</button>
        </div>
      ))}
    </div>
  );
}
