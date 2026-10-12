import type { AlwaysRule, HookStatus, OpenIn, TerminalApp } from "../types";

interface Props {
  hooks: HookStatus[];
  rules: AlwaysRule[];
  onInstall: (agent: string) => void;
  onUninstall: (agent: string) => void;
  onRemoveRule: (rule: AlwaysRule) => void;
  openIn?: OpenIn;
  /** Terminal apps found on this machine. */
  terminals?: TerminalApp[];
  onOpenIn?: (v: OpenIn) => void;
}

const projectOf = (cwd: string) => cwd.split(/[\\/]/).filter(Boolean).pop() ?? cwd;

const ability = (t: TerminalApp) =>
  t.realPrompts ? "real prompts" : t.runsCommand ? "messages as notes" : "messages as notes, paste to resume";

export function SettingsPanel({ hooks, rules, onInstall, onUninstall, onRemoveRule, openIn, terminals = [], onOpenIn }: Props) {
  const chosen = terminals.find((t) => t.id === openIn);
  return (
    <div className="settings">
      {openIn && onOpenIn && terminals.length > 0 && (
        <>
          <h3>Sessions</h3>
          <div className="row setting">
            <div>
              <label htmlFor="open-in"><strong>Open Claude sessions in</strong></label>
              <div className="meta">
                {chosen && !chosen.realPrompts
                  ? `${chosen.name} can't be typed into: messages from Peekaboo reach Claude as a note, not a real prompt.${chosen.runsCommand ? "" : " Resume opens a tab in the project and copies the command for you to paste."}`
                  : "Messages from Peekaboo arrive as real prompts."}
              </div>
            </div>
            <select id="open-in" aria-label="Open Claude sessions in" value={openIn} onChange={(e) => onOpenIn(e.target.value as OpenIn)}>
              {terminals.map((t) => <option key={t.id} value={t.id}>{`${t.name} — ${ability(t)}`}</option>)}
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
