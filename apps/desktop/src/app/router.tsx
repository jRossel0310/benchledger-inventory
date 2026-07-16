import { createRouter } from '@tanstack/react-router';

import { routeTree } from './routes';

export const router = createRouter({ routeTree });

// Registers this app's route tree as the default for every TanStack Router
// hook (`useParams`, `useNavigate`, `<Link>`, …) so they're fully typed
// without each call site importing a specific route object.
declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router;
  }
}
