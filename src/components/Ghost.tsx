export function Ghost({ size = 22, alert = false }: { size?: number; alert?: boolean }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" className={alert ? "ghost alert" : "ghost"} aria-label="Boo">
      <path d="M32 6C20 6 12 15 12 27v31l5-4 5 4 5-4 5 4 5-4 5 4 5-4 5 4V27C52 15 44 6 32 6z" fill="#f4f1ff" />
      <ellipse cx="25" cy="28" rx="3.5" ry="5" fill="#16131f" />
      <ellipse cx="39" cy="28" rx="3.5" ry="5" fill="#16131f" />
    </svg>
  );
}
