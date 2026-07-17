import { BinBrowser } from './BinBrowser';

/**
 * `/bins` route wrapper — mirrors `DashboardPage`'s split between the routed
 * page and its real content (`Dashboard`/`Dashboard`): keeps `routes.tsx`'s
 * existing import stable while the actual Bin browser screen (Phase 3
 * Task 8) lives in `BinBrowser.tsx`.
 */
export function BinsPage() {
  return <BinBrowser />;
}
