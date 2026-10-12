import { Ghost } from "./Ghost";

export const usageLevel = (pct: number) => (pct >= 80 ? "danger" : pct >= 50 ? "warn" : "ok");

interface Props {
  active: number;
  waiting: number;
  /** Active claudex account's weekly usage, when known. */
  weekPct?: number | null;
  onOpen: () => void;
}

export function Pill({ active, waiting, weekPct = null, onOpen }: Props) {
  return (
    <div className="pill" onMouseEnter={onOpen} onClick={onOpen}>
      <Ghost alert={waiting > 0} />
      <span className="pill-text">{waiting > 0 ? `${waiting} waiting` : active > 0 ? `${active} active` : "Peekaboo"}</span>
      {weekPct !== null && <span className={`usage ${usageLevel(weekPct)}`} title="Weekly usage (claudex)">{weekPct}%</span>}
    </div>
  );
}
