/** Boo, Peekaboo's ghost. Bobs gently; peeks when something needs the user. */
export function Ghost({ size = 20, alert = false }: { size?: number; alert?: boolean }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" className={alert ? "ghost alert" : "ghost"} aria-label="Boo">
      <path d="M32 5C19.5 5 11 14.5 11 27v30.5c0 1.6 1.9 2.4 3 1.3l3.6-3.5 4.4 4.1c.9.8 2.2.8 3 0l4.2-4 4.2 4c.8.8 2.2.8 3 0l4.2-4 4.4 4.1c.9.8 2.2.8 3 0L52 55.3c1.1 1.1 3 .3 3-1.3V27C53 14.5 44.5 5 32 5z" fill="#fff" />
      <ellipse className="eye" cx="24.5" cy="28" rx="3.6" ry="5.2" fill="#000" />
      <ellipse className="eye" cx="39.5" cy="28" rx="3.6" ry="5.2" fill="#000" />
    </svg>
  );
}
