import { useSearch } from '@tanstack/react-router';

import { History } from './History';

export function HistoryPage() {
  const { groupId } = useSearch({ from: '/history' });
  return <History initialGroupId={groupId ?? null} />;
}
