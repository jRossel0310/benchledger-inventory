/**
 * The part-detail body (Phase 3 Task 7, design direction §"Part detail is a
 * right-hand inspector drawer"): shared by `PartInspector` (the drawer, the
 * fast path from the Inventory table) and `PartDetailPage` (the full-page,
 * deep-link-friendly route) — everything about *what a part looks like* to
 * inspect lives here exactly once; the two callers differ only in chrome
 * (`onClose` present -> drawer close control + an "Open full page" escape
 * hatch; absent -> plain full-page render).
 *
 * Header: identity (category, display name, identity-attribute specs, bin —
 * all `--font-data`, matching the design direction's "identifiers read like
 * an instrument" thesis), a panel-size `StockGauge`, the four quantity
 * figures, and the primary actions (Add stock/Consume/Reserve/Check out),
 * each of which opens the shared `QuickAction` dialog (Task 5) preselected
 * to this part via `useQuickAction()` — this screen never duplicates that
 * dialog's ledger-op logic. Below the header, Radix `Tabs` switch between
 * the seven section components (`PartDetailOverview` etc.), each owning its
 * own data fetch so an unopened tab never fires a query it doesn't need.
 *
 * "Refresh product data" is a labeled stub (Phase 5 enrichment isn't built
 * yet) that responds with a toast explaining that, rather than doing
 * nothing silently — see the design direction's "never a dead control" rule
 * (also honored by the Dashboard's publish/backup strip).
 */

import * as Tabs from '@radix-ui/react-tabs';
import { Link } from '@tanstack/react-router';

import type { PartId } from '../../bindings.gen';
import { isStockLow, StockGauge } from '../../components/StockGauge';
import { useToast } from '../../components/Toast';
import {
  useAttributes,
  useCategories,
  useCategoryAttributeDefs,
  usePart,
  useStock,
} from '../../hooks/inventory';
import { errorMessage, formatQuantity } from '../../lib/format';
import type { QuickActionKind } from '../quick/quickActionConfig';
import { useQuickAction } from '../quick/QuickActionContext';
import { PartDetailDimensions } from './PartDetailDimensions';
import { PartDetailMetadata } from './PartDetailMetadata';
import { PartDetailOverview } from './PartDetailOverview';
import { PartDetailSpecifications } from './PartDetailSpecifications';
import { PartDetailSupplierListings } from './PartDetailSupplierListings';
import { PartDetailTransactions } from './PartDetailTransactions';
import { PartDetailVariants } from './PartDetailVariants';
import './PartDetail.css';

export interface PartDetailProps {
  partId: PartId;
  /** Present when rendered inside `PartInspector` (the drawer): renders a
   * close control and an "Open full page" link, both of which also close the
   * drawer. Absent for `PartDetailPage` (the standalone route), which has
   * nothing to close. */
  onClose?: () => void;
}

export function PartDetail({ partId, onClose }: PartDetailProps) {
  const partQuery = usePart(partId);
  const stockQuery = useStock(partId);
  const categoriesQuery = useCategories();
  const attributeDefsQuery = useCategoryAttributeDefs(partQuery.data?.category_id);
  const attributesQuery = useAttributes(partId);
  const quickAction = useQuickAction();
  const { toast } = useToast();

  if (partQuery.isPending) {
    return <p className="part-detail-status">Loading part…</p>;
  }
  if (partQuery.isError) {
    return (
      <p className="part-detail-status part-detail-status-error">
        Could not load this part: {errorMessage(partQuery.error)}
      </p>
    );
  }
  if (partQuery.data === null) {
    return (
      <div className="part-detail part-detail-not-found">
        <h2 className="part-detail-not-found-title">Part not found</h2>
        <p className="part-detail-not-found-description">
          This part no longer exists — it may have been deleted, or the link is out of date.
        </p>
        {onClose ? (
          <button type="button" className="part-detail-action-secondary" onClick={onClose}>
            Close
          </button>
        ) : null}
      </div>
    );
  }

  const part = partQuery.data;
  const categoryName =
    categoriesQuery.data?.find((c) => c.id === part.category_id)?.name ?? part.category_id;

  const attrByKey = new Map((attributesQuery.data ?? []).map(([key, text]) => [key, text]));
  const identitySpecs = (attributeDefsQuery.data ?? [])
    .filter((def) => def.identity && attrByKey.has(def.key))
    .sort((a, b) => a.display_order - b.display_order)
    .map((def) => attrByKey.get(def.key) as string);

  const stock = stockQuery.data;
  const low = stock ? isStockLow(stock.available, part.low_stock_threshold) : false;

  function openAction(kind: QuickActionKind) {
    quickAction.open({ kind, part: { id: partId, displayName: part.display_name } });
  }

  function handleRefresh() {
    toast({
      title: 'Enrichment arrives in Phase 5',
      description: 'Automatic product-data refresh from datasheets/suppliers isn’t built yet.',
      kind: 'warning',
    });
  }

  return (
    <div className="part-detail">
      <header className="part-detail-header">
        <div className="part-detail-header-top">
          <div>
            <p className="part-detail-eyebrow">{categoryName}</p>
            <h1 className="part-detail-title">{part.display_name}</h1>
            {identitySpecs.length > 0 ? (
              <p className="part-detail-specs">{identitySpecs.join(' · ')}</p>
            ) : null}
            <p className="part-detail-bin">{part.bin_label ?? 'No bin assigned'}</p>
          </div>
          {onClose ? (
            <button
              type="button"
              className="part-detail-close"
              aria-label="Close part detail"
              onClick={onClose}
            >
              ×
            </button>
          ) : null}
        </div>

        {stock ? (
          <>
            <div className="part-detail-gauge-row">
              <StockGauge
                available={stock.available}
                reserved={stock.reserved}
                checkedOut={stock.checked_out}
                unit={part.quantity_unit}
                lowThreshold={part.low_stock_threshold}
                size="panel"
              />
              {low ? <span className="part-detail-low-badge">Low stock</span> : null}
            </div>

            <dl className="part-detail-figures">
              <div>
                <dt>Available</dt>
                <dd>{formatQuantity(stock.available, part.quantity_unit)}</dd>
              </div>
              <div>
                <dt>Reserved</dt>
                <dd>{formatQuantity(stock.reserved, part.quantity_unit)}</dd>
              </div>
              <div>
                <dt>Checked out</dt>
                <dd>{formatQuantity(stock.checked_out, part.quantity_unit)}</dd>
              </div>
              <div>
                <dt>Current stock</dt>
                <dd>
                  {formatQuantity(
                    stock.available + stock.reserved + stock.checked_out,
                    part.quantity_unit,
                  )}
                </dd>
              </div>
            </dl>
          </>
        ) : null}

        <div className="part-detail-actions">
          <button
            type="button"
            className="part-detail-action-primary"
            onClick={() => openAction('receive')}
          >
            Add stock
          </button>
          <button
            type="button"
            className="part-detail-action-secondary"
            onClick={() => openAction('consume_available')}
          >
            Consume
          </button>
          <button
            type="button"
            className="part-detail-action-secondary"
            onClick={() => openAction('reserve')}
          >
            Reserve
          </button>
          <button
            type="button"
            className="part-detail-action-secondary"
            onClick={() => openAction('check_out')}
          >
            Check out
          </button>
          <Link
            to="/inventory/$partId/edit"
            params={{ partId }}
            className="part-detail-action-secondary"
            onClick={() => onClose?.()}
          >
            Edit
          </Link>
          <button type="button" className="part-detail-action-secondary" onClick={handleRefresh}>
            Refresh product data
          </button>
          {onClose ? (
            <Link
              to="/inventory/$partId"
              params={{ partId }}
              className="part-detail-action-secondary"
              onClick={() => onClose()}
            >
              Open full page
            </Link>
          ) : null}
        </div>
      </header>

      <Tabs.Root className="part-detail-tabs" defaultValue="overview">
        <Tabs.List className="part-detail-tabs-list" aria-label="Part detail sections">
          <Tabs.Trigger className="part-detail-tabs-trigger" value="overview">
            Overview
          </Tabs.Trigger>
          <Tabs.Trigger className="part-detail-tabs-trigger" value="specifications">
            Specifications
          </Tabs.Trigger>
          <Tabs.Trigger className="part-detail-tabs-trigger" value="dimensions">
            Dimensions
          </Tabs.Trigger>
          <Tabs.Trigger className="part-detail-tabs-trigger" value="variants">
            Variants
          </Tabs.Trigger>
          <Tabs.Trigger className="part-detail-tabs-trigger" value="listings">
            Supplier listings
          </Tabs.Trigger>
          <Tabs.Trigger className="part-detail-tabs-trigger" value="transactions">
            Transactions
          </Tabs.Trigger>
          <Tabs.Trigger className="part-detail-tabs-trigger" value="metadata">
            Metadata
          </Tabs.Trigger>
        </Tabs.List>

        <Tabs.Content className="part-detail-tabs-content" value="overview">
          <PartDetailOverview part={part} />
        </Tabs.Content>
        <Tabs.Content className="part-detail-tabs-content" value="specifications">
          <PartDetailSpecifications partId={partId} categoryId={part.category_id} />
        </Tabs.Content>
        <Tabs.Content className="part-detail-tabs-content" value="dimensions">
          <PartDetailDimensions partId={partId} />
        </Tabs.Content>
        <Tabs.Content className="part-detail-tabs-content" value="variants">
          <PartDetailVariants partId={partId} />
        </Tabs.Content>
        <Tabs.Content className="part-detail-tabs-content" value="listings">
          <PartDetailSupplierListings partId={partId} />
        </Tabs.Content>
        <Tabs.Content className="part-detail-tabs-content" value="transactions">
          <PartDetailTransactions partId={partId} unit={part.quantity_unit} />
        </Tabs.Content>
        <Tabs.Content className="part-detail-tabs-content" value="metadata">
          <PartDetailMetadata part={part} />
        </Tabs.Content>
      </Tabs.Root>
    </div>
  );
}
