import { Ghost } from "./Ghost";

export function Pill({ active, waiting, onOpen }: { active: number; waiting: number; onOpen: () => void }) {
  return (
    <div className="pill" onMouseEnter={onOpen} onClick={onOpen}>
      <Ghost alert={waiting > 0} />
      <span className="pill-text">{waiting > 0 ? `${waiting} waiting` : active > 0 ? `${active} active` : "Peekaboo"}</span>
    </div>
  );
}
