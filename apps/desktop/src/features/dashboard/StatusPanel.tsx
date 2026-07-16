import { useEffect, useState } from 'react';
import { commands, type AppStatus } from '../../bindings';

export function StatusPanel() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    commands.appStatus().then((result) => {
      if (result.status === 'ok') {
        setStatus(result.data);
      } else {
        setError(result.error.message);
      }
    }, (e: unknown) => setError(String(e)));
  }, []);

  if (error) {
    return <div className="panel">Could not load application status: {error}</div>;
  }
  if (!status) {
    return <div className="panel">Loading…</div>;
  }
  return (
    <dl className="panel">
      <dt>Application version</dt>
      <dd>{status.appVersion}</dd>
      <dt>Database</dt>
      <dd>schema v{status.schemaVersion}</dd>
      <dt>Data directory</dt>
      <dd>{status.dataDir}</dd>
    </dl>
  );
}
