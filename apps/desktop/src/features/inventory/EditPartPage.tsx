import { useParams } from '@tanstack/react-router';

import type { PartId } from '../../bindings.gen';
import { PartForm } from '../part/PartForm';

/**
 * `/inventory/$partId/edit` — Task 6's category-adaptive form in edit mode
 * (`PartForm` with the route's `partId`). The create route
 * (`/inventory/new`) renders the same component with no `partId`.
 */
export function EditPartPage() {
  const { partId } = useParams({ from: '/inventory/$partId/edit' });
  return <PartForm partId={partId as PartId} />;
}
