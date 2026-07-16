/**
 * Minimal monochrome 16x16 line icons for the left rail and top command bar.
 * Hand-rolled inline SVG rather than an icon library dependency — the
 * "bench instrument" direction wants a handful of quiet, consistent glyphs,
 * not a general-purpose icon set. All strokes use `currentColor` so they
 * inherit the rail item's text color (including the active/hover states).
 */

import type { SVGProps } from 'react';

function Icon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...props}
    />
  );
}

export function DashboardIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <Icon {...props}>
      <rect x="2" y="2" width="5" height="5" rx="0.5" />
      <rect x="9" y="2" width="5" height="5" rx="0.5" />
      <rect x="2" y="9" width="5" height="5" rx="0.5" />
      <rect x="9" y="9" width="5" height="5" rx="0.5" />
    </Icon>
  );
}

export function InventoryIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <Icon {...props}>
      <line x1="2.5" y1="4" x2="13.5" y2="4" />
      <line x1="2.5" y1="8" x2="13.5" y2="8" />
      <line x1="2.5" y1="12" x2="13.5" y2="12" />
      <circle cx="2.5" cy="4" r="0.6" fill="currentColor" stroke="none" />
      <circle cx="2.5" cy="8" r="0.6" fill="currentColor" stroke="none" />
      <circle cx="2.5" cy="12" r="0.6" fill="currentColor" stroke="none" />
    </Icon>
  );
}

export function BinsIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <Icon {...props}>
      <rect x="2.5" y="3" width="11" height="10" rx="0.5" />
      <line x1="2.5" y1="7" x2="13.5" y2="7" />
      <line x1="7.7" y1="3" x2="7.7" y2="13" />
    </Icon>
  );
}

export function ProjectsIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <Icon {...props}>
      <path d="M2 4.5c0-.55.45-1 1-1h3l1.3 1.5H13c.55 0 1 .45 1 1V11c0 .55-.45 1-1 1H3c-.55 0-1-.45-1-1V4.5Z" />
    </Icon>
  );
}

export function OrdersIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <Icon {...props}>
      <path d="M4 2h6.5L13 4.5V13a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1Z" />
      <path d="M10 2v2.5H13" />
      <line x1="5" y1="8" x2="10" y2="8" />
      <line x1="5" y1="10.5" x2="8.5" y2="10.5" />
    </Icon>
  );
}

export function HistoryIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <Icon {...props}>
      <circle cx="8" cy="8.5" r="5.2" />
      <path d="M8 5.5V8.5L10.2 10" />
      <path d="M3.6 4.2 2.8 2.4l1.9.5" />
    </Icon>
  );
}

export function SettingsIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <Icon {...props}>
      <circle cx="8" cy="8" r="2.1" />
      <path d="M8 2.3v1.5M8 12.2v1.5M13.7 8h-1.5M3.8 8H2.3M12 4l-1.05 1.05M5.05 10.95 4 12M12 12l-1.05-1.05M5.05 5.05 4 4" />
    </Icon>
  );
}

export function SearchIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <Icon {...props}>
      <circle cx="7" cy="7" r="4.5" />
      <line x1="10.3" y1="10.3" x2="14" y2="14" />
    </Icon>
  );
}
