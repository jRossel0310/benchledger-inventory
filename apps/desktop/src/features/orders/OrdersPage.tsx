import { OrdersList } from './OrdersList';

/**
 * `/orders` route wrapper — the Phase 3 stub is retired (Phase 5d Task 2)
 * now that `OrdersList` is real. Mirrors `HistoryPage`'s thin
 * route-wrapper-around-a-reusable-body pattern.
 */
export function OrdersPage() {
  return <OrdersList />;
}
