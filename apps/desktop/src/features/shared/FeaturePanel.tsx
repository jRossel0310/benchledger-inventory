import type { ReactNode } from 'react';

interface FeaturePanelProps {
  /** Small-caps section label above the title, e.g. "Dashboard". */
  eyebrow: string;
  title: string;
  description: string;
  children?: ReactNode;
}

/**
 * Consistent titled-panel shell for a route's content: an eyebrow label, a
 * title, a description, and optional real content below. Used both by the
 * Phase 3 route stubs (title + description only) and, later, by screens
 * that fill in `children`.
 */
export function FeaturePanel({ eyebrow, title, description, children }: FeaturePanelProps) {
  return (
    <section className="feature-panel">
      <p className="feature-panel-eyebrow">{eyebrow}</p>
      <h1 className="feature-panel-title">{title}</h1>
      <p className="feature-panel-description">{description}</p>
      {children}
    </section>
  );
}
