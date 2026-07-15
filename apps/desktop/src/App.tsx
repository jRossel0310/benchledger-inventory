import { StatusPanel } from './features/dashboard/StatusPanel';

export function App() {
  return (
    <div className="shell">
      <aside className="sidebar">
        <h1>Electronics Inventory</h1>
        <span className="nav-item">Dashboard</span>
      </aside>
      <main className="content">
        <StatusPanel />
      </main>
    </div>
  );
}
