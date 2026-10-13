import { Ghost } from "./Ghost";

export const usageLevel = (pct: number) => (pct >= 80 ? "danger" : pct >= 50 ? "warn" : "ok");

function Ring({ pct }: { pct: number }) {
  const r = 6.5;
  const c = 2 * Math.PI * r;
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" className={`ring ${usageLevel(pct)}`} aria-hidden="true">
      <circle cx="8" cy="8" r={r} className="track" />
      <circle cx="8" cy="8" r={r} className="value" strokeDasharray={`${(c * Math.min(pct, 100)) / 100} ${c}`} />
    </svg>
  );
}

interface Props {
  active: number;
  waiting: number;
  /** Active claudex account's 5-hour session usage, when known. */
  sessionPct?: number | null;
  /** Active claudex account's weekly usage, when known. */
  weekPct?: number | null;
  /** Width of the camera notch; 0 renders a floating pill instead of an island around it. */
  notchWidth?: number;
  onOpen: () => void;
}

/** Collapsed island: Boo in the left ear, status in the right, the camera in between. */
function Usage({ pct, label, title }: { pct: number; label: string; title: string }) {
  return (
    <span className="usage-wrap" title={`${title}: ${pct}%`}>
      <Ring pct={pct} />
      <span className="usage-label">{label}</span>
      <span className={`usage ${usageLevel(pct)}`}>{pct}%</span>
    </span>
  );
}

export function Pill({ active, waiting, sessionPct = null, weekPct = null, notchWidth = 0, onOpen }: Props) {
  return (
    <div className={`island ${notchWidth ? "notched" : "floating"} ${waiting ? "attention" : ""}`} onClick={onOpen} onMouseEnter={onOpen}>
      <div className="ear left">
        <Ghost size={18} alert={waiting > 0} />
        {!notchWidth && <span className="brand">Peekaboo</span>}
        {active > 0 && !waiting && <span className="count" title={`${active} active`}>{active}</span>}
      </div>
      {notchWidth > 0 && <div className="cutout" style={{ width: notchWidth }} />}
      <div className="ear right">
        {waiting > 0 ? (
          <span className="waiting">{waiting} waiting</span>
        ) : (
          <>
            {sessionPct !== null && <Usage pct={sessionPct} label="5h" title="5-hour session limit" />}
            {weekPct !== null && <Usage pct={weekPct} label="W" title="Weekly limit" />}
          </>
        )}
      </div>
    </div>
  );
}
