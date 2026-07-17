/**
 * The part-detail inspector: a right-hand drawer over the current screen
 * (Phase 3 Task 7, design direction §"Part detail is a right-hand inspector
 * drawer") — a Radix `Dialog` styled as a slide-over rather than a centered
 * modal, so it reads as a properties panel over the Inventory table rather
 * than an interruption. Wraps the shared `PartDetail` body, which does all
 * the real work (data, tabs, actions); this component only owns the dialog
 * chrome (overlay, slide-over positioning, the `onClose` wiring `PartDetail`
 * uses for its close control and "Open full page" link).
 */

import * as Dialog from '@radix-ui/react-dialog';

import type { PartId } from '../../bindings.gen';
import { PartDetail } from './PartDetail';
import './PartInspector.css';

export interface PartInspectorProps {
  partId: PartId;
  onClose: () => void;
}

export function PartInspector({ partId, onClose }: PartInspectorProps) {
  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="part-inspector-overlay" />
        <Dialog.Content className="part-inspector-content" aria-label="Part detail">
          {/* `PartDetail`'s own header already renders the part's name
           * prominently as a heading — the Dialog title/description exist
           * only to satisfy Radix's accessibility contract without visibly
           * duplicating that heading. */}
          <Dialog.Title className="part-inspector-visually-hidden">Part detail</Dialog.Title>
          <Dialog.Description className="part-inspector-visually-hidden">
            Inspect and act on this part without leaving the inventory list.
          </Dialog.Description>
          <PartDetail partId={partId} onClose={onClose} />
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
