/**
 * Read-only part detail view -- everything the snapshot publishes about one
 * part: header (name, category, bin, the three stock figures + low badge,
 * a larger version of the table's segmented stock bar), description/public
 * notes/tags, specifications (attribute display values with the normalized
 * value + canonical unit as a subtle aside), dimensions, variants
 * (manufacturer/MPN/package/lifecycle/datasheet/product links), supplier
 * part numbers (supplier + SKU only -- the snapshot deliberately carries no
 * prices), and project associations cross-linked to `#/projects`.
 *
 * Empty sections are omitted rather than rendered as placeholders; an
 * unknown part id gets an honest not-found panel with a back link.
 */

import type { Snapshot, SnapshotPart } from '@ei/shared';
import './PartDetail.css';
import { formatQuantity } from './format';

function StockBar({ part }: { part: SnapshotPart }) {
  const { availableMilli, reservedMilli, checkedOutMilli } = part.stock;
  const total = availableMilli + reservedMilli + checkedOutMilli;
  const pct = (value: number) => (total > 0 ? (value / total) * 100 : 0);
  return (
    <div className="part-stock-bar" aria-hidden="true">
      <span className="part-stock-bar-available" style={{ width: `${pct(availableMilli)}%` }} />
      <span className="part-stock-bar-reserved" style={{ width: `${pct(reservedMilli)}%` }} />
      <span className="part-stock-bar-checked-out" style={{ width: `${pct(checkedOutMilli)}%` }} />
    </div>
  );
}

function StockFigure({ label, milli, unit }: { label: string; milli: number; unit: string }) {
  return (
    <div className="part-stock-figure">
      <span className="part-stock-label">{label}</span>
      <span className="part-stock-value">{formatQuantity(milli, unit)}</span>
    </div>
  );
}

function ExternalLink({ href, children }: { href: string; children: string }) {
  return (
    <a className="part-link" href={href} target="_blank" rel="noopener noreferrer">
      {children}
    </a>
  );
}

export function PartDetail({ snapshot, partId }: { snapshot: Snapshot; partId: string }) {
  const part = snapshot.parts.find((p) => p.id === partId);
  if (part === undefined) {
    return (
      <div className="empty">
        <h2>Part not found</h2>
        <p>No part with this id exists in the published snapshot.</p>
        <a className="part-back" href="#/">
          ← Back to inventory
        </a>
      </div>
    );
  }

  const projects = snapshot.projects.filter((project) => project.partIds.includes(part.id));
  const listings = part.variants.flatMap((variant) =>
    variant.listings.map((listing) => ({ variant, listing })),
  );

  return (
    <article className="part-detail">
      <a className="part-back" href="#/">
        ← All parts
      </a>
      <header className="part-header">
        <div className="part-title-line">
          <h2 className="part-title">{part.displayName}</h2>
          {part.lowStock && <span className="part-low">Low stock</span>}
        </div>
        <p className="part-meta">
          <span>{part.category}</span>
          <span className="part-meta-bin">Bin: {part.bin ?? 'unassigned'}</span>
        </p>
        <div className="part-stock-row">
          <StockFigure
            label="Available"
            milli={part.stock.availableMilli}
            unit={part.quantityUnit}
          />
          <StockFigure label="Reserved" milli={part.stock.reservedMilli} unit={part.quantityUnit} />
          <StockFigure
            label="Checked out"
            milli={part.stock.checkedOutMilli}
            unit={part.quantityUnit}
          />
        </div>
        <StockBar part={part} />
      </header>

      {(part.description !== '' || part.publicNotes !== '' || part.tags.length > 0) && (
        <section className="part-section">
          <h3 className="part-section-title">About</h3>
          {part.description !== '' && <p className="part-description">{part.description}</p>}
          {part.publicNotes !== '' && <p className="part-notes">{part.publicNotes}</p>}
          {part.tags.length > 0 && (
            <div className="part-tags">
              {part.tags.map((tag) => (
                <span key={tag} className="part-tag">
                  {tag}
                </span>
              ))}
            </div>
          )}
        </section>
      )}

      {part.attributes.length > 0 && (
        <section className="part-section">
          <h3 className="part-section-title">Specifications</h3>
          <table className="part-table">
            <tbody>
              {part.attributes.map((attr) => (
                <tr key={attr.key}>
                  <th scope="row">{attr.label}</th>
                  <td className="part-data">{attr.originalText}</td>
                  <td className="part-normalized">
                    {attr.normalizedValue !== null &&
                      attr.canonicalUnit !== null &&
                      `${attr.normalizedValue} ${attr.canonicalUnit}`}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}

      {part.dimensions.length > 0 && (
        <section className="part-section">
          <h3 className="part-section-title">Dimensions</h3>
          <table className="part-table">
            <tbody>
              {part.dimensions.map((dim) => (
                <tr key={`${dim.group}:${dim.name}`}>
                  <th scope="row">{dim.name}</th>
                  <td className="part-data">
                    {dim.valueNum} {dim.displayUnit}
                  </td>
                  <td className="part-normalized">{dim.group}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}

      {part.variants.length > 0 && (
        <section className="part-section">
          <h3 className="part-section-title">Variants</h3>
          <ul className="part-list">
            {part.variants.map((variant) => (
              <li key={variant.id} className="part-list-item">
                <div className="part-variant-line">
                  <span>{variant.manufacturer}</span>
                  <span className="part-data">{variant.mpn}</span>
                  {variant.package !== null && <span className="part-data">{variant.package}</span>}
                  {variant.lifecycle !== null && (
                    <span className="part-lifecycle">{variant.lifecycle}</span>
                  )}
                  {variant.isPreferred && <span className="part-preferred">Preferred</span>}
                </div>
                {(variant.datasheetUrl !== null || variant.productUrl !== null) && (
                  <div className="part-variant-links">
                    {variant.datasheetUrl !== null && (
                      <ExternalLink href={variant.datasheetUrl}>Datasheet</ExternalLink>
                    )}
                    {variant.productUrl !== null && (
                      <ExternalLink href={variant.productUrl}>Product page</ExternalLink>
                    )}
                  </div>
                )}
              </li>
            ))}
          </ul>
        </section>
      )}

      {listings.length > 0 && (
        <section className="part-section">
          <h3 className="part-section-title">Supplier part numbers</h3>
          <ul className="part-list">
            {listings.map(({ variant, listing }) => (
              <li
                key={`${variant.id}:${listing.supplier}:${listing.supplierSku}`}
                className="part-list-item"
              >
                <div className="part-variant-line">
                  <span>{listing.supplier}</span>
                  <span className="part-data">{listing.supplierSku}</span>
                  {listing.packaging !== null && (
                    <span className="part-packaging">{listing.packaging}</span>
                  )}
                  {listing.productUrl !== null && (
                    <ExternalLink href={listing.productUrl}>Product page</ExternalLink>
                  )}
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}

      {projects.length > 0 && (
        <section className="part-section">
          <h3 className="part-section-title">Used in projects</h3>
          <ul className="part-list">
            {projects.map((project) => (
              <li key={project.id} className="part-list-item">
                <a className="part-project-link" href="#/projects">
                  {project.name}
                </a>
              </li>
            ))}
          </ul>
        </section>
      )}
    </article>
  );
}
