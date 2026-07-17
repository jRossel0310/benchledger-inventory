import { PartForm } from '../part/PartForm';

/**
 * `/inventory/new` — the Ctrl+K palette's "Create part" action routes here.
 * Task 6's category-adaptive create form (`PartForm` with no `partId`, i.e.
 * create mode). The `/inventory/$partId/edit` route renders the same
 * component in edit mode.
 */
export function CreatePartPage() {
  return <PartForm />;
}
