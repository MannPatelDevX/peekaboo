import type { AlwaysRule, HookStatus } from "../types";

interface Props {
  hooks: HookStatus[];
  rules: AlwaysRule[];
  onInstall: (agent: string) => void;
  onUninstall: (agent: string) => void;
  onRemoveRule: (rule: AlwaysRule) => void;
}

export function SettingsPanel({ hooks, rules, onInstall, onUninstall, onRemoveRule }: Props) {
  return (
    <div className="settings">
      <h3>Agents</h3>
      {hooks.map((h) => (
        <div key={h.agent} className="row setting">
          <div>
            <strong>{h.agent}</strong>
            <div className="meta">{h.error ?? h.configPath}</div>
          </div>
          {h.installed ? (
            <button onClick={() => onUninstall(h.agent)}>Uninstall</button>
          ) : (
            <button className="primary" disabled={!!h.error} onClick={() => onInstall(h.agent)}>Install</button>
          )}
        </div>
      ))}
      <h3>Always allowed</h3>
      {rules.length === 0 && <p className="empty">Nothing yet. Choose "Always" on a request to add one.</p>}
      {rules.map((r) => (
        <div key={`${r.agent}:${r.tool}`} className="row setting">
          <span>{r.agent} · {r.tool}</span>
          <button aria-label={`Remove ${r.tool}`} onClick={() => onRemoveRule(r)}>✕</button>
        </div>
      ))}
    </div>
  );
}
