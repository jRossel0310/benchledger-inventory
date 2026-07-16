import { Link, Outlet } from '@tanstack/react-router';
import { useEffect, useRef, type ReactNode } from 'react';

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
 */
export function AppShell() {
  const searchRef = useRef<HTMLInputElement>(null);

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
