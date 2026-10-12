// Minimal 16px stroke icons in an SF Symbols-like style.
import type { SVGProps } from "react";

const Svg = ({ children, ...p }: SVGProps<SVGSVGElement>) => (
  <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5"
    strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" {...p}>{children}</svg>
);

export const AgentsIcon = () => <Svg><path d="M8 1.8l1.4 3.6 3.8.3-2.9 2.4.9 3.7L8 9.8l-3.2 2 .9-3.7-2.9-2.4 3.8-.3z" /></Svg>;
export const SessionsIcon = () => <Svg><rect x="1.8" y="2.8" width="12.4" height="10.4" rx="2.2" /><path d="M4.6 6.3l2 1.7-2 1.7M8.4 10h3" /></Svg>;
export const AccountsIcon = () => <Svg><path d="M2.6 11.5a5.6 5.6 0 1 1 10.8 0" /><path d="M8 9.6l2.6-3" /></Svg>;
export const SettingsIcon = () => <Svg><circle cx="8" cy="8" r="2.2" /><path d="M8 1.6v1.6M8 12.8v1.6M14.4 8h-1.6M3.2 8H1.6M12.5 3.5l-1.1 1.1M4.6 11.4l-1.1 1.1M12.5 12.5l-1.1-1.1M4.6 4.6L3.5 3.5" /></Svg>;
export const RefreshIcon = () => <Svg><path d="M13.2 6.4A5.4 5.4 0 0 0 3.1 5M2.8 9.6A5.4 5.4 0 0 0 12.9 11" /><path d="M13.4 2.8v3.6H9.8M2.6 13.2V9.6h3.6" /></Svg>;
export const BranchIcon = () => <Svg width="12" height="12"><circle cx="4.5" cy="3.5" r="1.5" /><circle cx="4.5" cy="12.5" r="1.5" /><circle cx="11.5" cy="5.5" r="1.5" /><path d="M4.5 5v6M11.5 7c0 2.5-3 2.5-7 4" /></Svg>;
