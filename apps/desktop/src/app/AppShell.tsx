import { Link, Outlet, useNavigate, useRouterState } from '@tanstack/react-router';
import { useEffect, useRef, useState, type ReactNode } from 'react';

import { useDebouncedCallback } from '../hooks/useDebouncedCallback';
import {
  BinsIcon,
  DashboardIcon,
  HistoryIcon,
  InventoryIcon,
  OrdersIcon,
  ProjectsIcon,
  SearchIcon,
  SettingsIcon,
} from './icons';

const SEARCH_DEBOUNCE_MS = 200;

interface NavItem {
  to: string;
  label: string;
  icon: ReactNode;
  exact?: boolean;
}

const NAV_ITEMS: NavItem[] = [
  { to: '/', label: 'Dashboard', icon: <DashboardIcon />, exact: true },
  { to: '/inventory', label: 'Inventory', icon: <InventoryIcon /> },
  { to: '/bins', label: 'Bins', icon: <BinsIcon /> },
  { to: '/projects', label: 'Projects', icon: <ProjectsIcon /> },
  { to: '/orders', label: 'Orders', icon: <OrdersIcon /> },
  { to: '/history', label: 'History', icon: <HistoryIcon /> },
  { to: '/settings', label: 'Settings', icon: <SettingsIcon /> },
];

/**
 * The persistent app frame: a left rail (Dashboard/Inventory/Bins/Projects/
 * Orders/History/Settings) and a top command bar (global search + the
 * `Ctrl+K` command-palette affordance) around a routed `<Outlet/>`. The
 * command palette itself is built in Phase 3 Task 5; for now `Ctrl+K`
 * focuses the search input, which is the honest behavior until the palette
 * exists.
 *
 * The search input is lifted into the Inventory route's `q` search param
 * (Phase 3 Task 4): typing here navigates to `/inventory` and updates `q`
 * (via `replace`, so clearing/refining a search doesn't spam browser
 * history), and — while already on `/inventory` — reflects that route's
 * current `q` back into the box (so a Dashboard card link like `?q=low
 * stock` shows up here too, not just in the table). This makes the one
 * search box genuinely global, matching the design direction's "search is
 * never more than a glance away."
 *
 * The box itself updates on every keystroke (`searchValue`, controlled,
 * immediate), but the `q` route-param write is debounced ~200ms
 * (`useDebouncedCallback`): each write mints a new `keys.search(query)`
 * cache key (`hooks/inventory.ts`), and writing it on every keystroke was
 * both spamming history entries and thrashing the query cache — flipping
 * `useInventorySearch` to "no data yet" on the Inventory table for every
 * character typed. Clearing the box (its native `type="search"` × button)
 * flushes the write immediately rather than leaving a stale, still-filtered
 * `q` live for the debounce window.
 */
export function AppShell() {
  const searchRef = useRef<HTMLInputElement>(null);
  const navigate = useNavigate();
  const location = useRouterState({ select: (state) => state.location });
  const routeQuery =
    location.pathname === '/inventory' ? String((location.search as { q?: unknown }).q ?? '') : '';
  const [searchValue, setSearchValue] = useState(routeQuery);

  useEffect(() => {
    setSearchValue(routeQuery);
  }, [routeQuery]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        searchRef.current?.focus();
      }
    }
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  const debouncedNavigateToQuery = useDebouncedCallback((value: string) => {
    void navigate({ to: '/inventory', search: { q: value }, replace: true });
  }, SEARCH_DEBOUNCE_MS);

  function handleSearchChange(value: string) {
    setSearchValue(value);
    // A cleared box should read as cleared immediately, not stay filtered
    // for the debounce window.
    if (value.trim().length === 0) {
      debouncedNavigateToQuery.flush(value);
    } else {
      debouncedNavigateToQuery.run(value);
    }
  }

  return (
    <div className="shell">
      <aside className="rail" aria-label="Main navigation">
        <div className="rail-brand">Electronics Inventory</div>
        <nav className="rail-nav">
          {NAV_ITEMS.map((item) => (
            <Link
              key={item.to}
              to={item.to}
              className="rail-item"
              activeOptions={{ exact: item.exact ?? false }}
              activeProps={{ className: 'rail-item rail-item-active' }}
            >
              <span className="rail-item-icon">{item.icon}</span>
              <span className="rail-item-label">{item.label}</span>
            </Link>
          ))}
        </nav>
      </aside>
      <div className="shell-main">
        <header className="command-bar">
          <div className="command-bar-search">
            <SearchIcon className="command-bar-icon" />
            <input
              ref={searchRef}
              type="search"
              className="command-bar-input"
              placeholder="Search parts, bins, MPNs…"
              aria-label="Search inventory"
              value={searchValue}
              onChange={(event) => handleSearchChange(event.target.value)}
            />
            <kbd className="command-bar-kbd">Ctrl K</kbd>
          </div>
        </header>
        <main className="shell-content">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
