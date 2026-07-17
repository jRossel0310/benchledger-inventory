/**
 * The shared keyboard-first quick-action dialog (Phase 3 Task 5, design
 * direction §"Ctrl+K command palette is the keyboard-first spine"): search/
 * confirm a part, enter a quantity with a live "remaining after" preview,
 * fill in whatever the action needs (a project, a reason note, optional
 * receive details), confirm — which calls the matching `useApplyLedgerOp`
 * mutation and toasts the effect ("Received 10", "Reserved 5 for Blinky
 * Board"). One component drives every ledger-backed quick action
 * (Add stock/Consume/Reserve/Release/Check out/Return); which fields it
 * shows is entirely data-driven off `quickActionConfig(request.kind)`.
 *
 * Opened three ways, all converging on this one component so its behavior
 * (and this test suite) never diverges per caller: the Ctrl+K
 * `CommandPalette` (no preselected part — the search step runs first), a row
 * action (`RowActions.tsx`, preselected part, skips the search step), and
 * — once Task 7 lands — the part-detail inspector. `QuickActionContext`
 * (`useQuickAction()`) is the shared "open a request" entry point every
 * caller uses instead of importing this component directly.
 */

import * as Dialog from '@radix-ui/react-dialog';
import { Command } from 'cmdk';
import { useMemo, useRef, useState, type FormEvent } from 'react';

import type { PartId, ProjectId, ProjectRef, SearchHit } from '../../bindings.gen';
import {
  NumberField,
  SelectField,
  TextField,
  type SelectFieldOption,
} from '../../components/Field';
import { useToast } from '../../components/Toast';
import {
  useApplyLedgerOp,
  useCreateProject,
  useProjects,
  useSearch,
  useStock,
} from '../../hooks/inventory';
import { errorHint, type CommandError } from '../../lib/format';
import {
  buildLedgerOp,
  composeReceiveNote,
  quickActionConfig,
  quickActionToastTitle,
  type QuickActionKind,
} from './quickActionConfig';
import { formatRemainingAfter } from './remainingAfter';
import './QuickAction.css';

/** A part already chosen before the dialog opens (a row action, or Task 7's
 * part-detail inspector) — skips the search step straight to quantity.
 * `unit` defaults to `'each'`: `SearchHit` (the shape row actions and this
 * dialog's own search step both work from) doesn't carry a part's
 * `quantity_unit` — see `InventoryTable.tsx`/`RowActions.tsx`. */
export interface QuickActionPart {
  id: PartId;
  displayName: string;
  unit?: string;
}

export interface QuickActionRequest {
  kind: QuickActionKind;
  part?: QuickActionPart;
}

export interface QuickActionProps {
  request: QuickActionRequest;
  onClose: () => void;
}

const CREATE_PROJECT_SENTINEL = '__create_new_project__';

function projectSelectOptions(projects: ProjectRef[], required: boolean): SelectFieldOption[] {
  return [
    { value: '', label: required ? 'Select a project…' : 'No project' },
    ...projects.map((p) => ({ value: p.id, label: p.name })),
    { value: CREATE_PROJECT_SENTINEL, label: 'Create new project…' },
  ];
}

export function QuickAction({ request, onClose }: QuickActionProps) {
  const config = quickActionConfig(request.kind);
  const { toast } = useToast();

  const [part, setPart] = useState<QuickActionPart | null>(request.part ?? null);
  const [partQuery, setPartQuery] = useState('');
  const [quantity, setQuantity] = useState<number | ''>('');
  const [note, setNote] = useState('');
  const [projectId, setProjectId] = useState<ProjectId | null>(null);
  const [creatingProject, setCreatingProject] = useState(false);
  const [newProjectName, setNewProjectName] = useState('');
  const [justCreatedProject, setJustCreatedProject] = useState<ProjectRef | null>(null);
  const [showDetails, setShowDetails] = useState(false);
  const [supplier, setSupplier] = useState('');
  const [order, setOrder] = useState('');
  const [date, setDate] = useState('');
  const [cost, setCost] = useState('');
  const [submitError, setSubmitError] = useState<CommandError | null>(null);

  const pendingProjectNameRef = useRef('');

  const partsSearch = useSearch(partQuery);
  const stockQuery = useStock(part?.id);
  const projectsQuery = useProjects();

  const unit = part?.unit ?? 'each';
  const quantityMilli = quantity === '' ? 0 : Math.round(quantity * 1000);

  const createProject = useCreateProject({
    onDone: (error, data) => {
      if (error) {
        setSubmitError(error);
        return;
      }
      if (data) {
        const created: ProjectRef = { id: data, name: pendingProjectNameRef.current };
        setJustCreatedProject(created);
        setProjectId(created.id);
        setCreatingProject(false);
        setNewProjectName('');
      }
    },
  });

  const applyOp = useApplyLedgerOp({
    onDone: (error, data) => {
      if (error) {
        setSubmitError(error);
        toast({
          title: `Could not ${config.submitLabel.toLowerCase()}`,
          description: errorHint(error.code) ?? error.message,
          kind: 'error',
        });
        return;
      }
      if (data) {
        toast({
          title: quickActionToastTitle(request.kind, quantityMilli, unit, projectName),
          kind: 'success',
        });
      }
      onClose();
    },
  });

  // Merge in a project just created inline: `useCreateProject` invalidates
  // the `projects` query, but rendering the freshly picked project
  // immediately (rather than waiting on that refetch) keeps the select and
  // the eventual toast's project name correct without a visible flash.
  const projects = useMemo(() => {
    const base = projectsQuery.data ?? [];
    if (justCreatedProject && !base.some((p) => p.id === justCreatedProject.id)) {
      return [...base, justCreatedProject];
    }
    return base;
  }, [projectsQuery.data, justCreatedProject]);

  const projectName = projectId ? (projects.find((p) => p.id === projectId)?.name ?? null) : null;

  const remainingAfterText =
    part && stockQuery.data && quantityMilli > 0
      ? formatRemainingAfter(request.kind, stockQuery.data, quantityMilli, unit)
      : null;

  const submitDisabled =
    !part ||
    quantityMilli <= 0 ||
    (config.project === 'required' && !projectId) ||
    applyOp.isPending;

  function pickPart(hit: SearchHit) {
    setPart({ id: hit.part_id, displayName: hit.display_name });
  }

  function handleProjectSelectChange(raw: string) {
    if (raw === CREATE_PROJECT_SENTINEL) {
      setCreatingProject(true);
      return;
    }
    setCreatingProject(false);
    setProjectId(raw === '' ? null : raw);
  }

  function submitNewProject() {
    const name = newProjectName.trim();
    if (!name || createProject.isPending) return;
    pendingProjectNameRef.current = name;
    createProject.mutate(name);
  }

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!part || quantityMilli <= 0) return;
    if (config.project === 'required' && !projectId) return;
    setSubmitError(null);
    const finalNote =
      request.kind === 'receive' ? composeReceiveNote({ note, supplier, order, date, cost }) : note;
    const op = buildLedgerOp({
      kind: request.kind,
      partId: part.id,
      quantityMilli,
      note: finalNote,
      projectId,
    });
    applyOp.mutate(op);
  }

  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="quick-action-overlay" />
        <Dialog.Content className="quick-action-content">
          <Dialog.Title className="quick-action-title">{config.label}</Dialog.Title>
          <Dialog.Description className="quick-action-description">
            {part ? part.displayName : `Search for a part to ${config.label.toLowerCase()}.`}
          </Dialog.Description>

          {!part ? (
            <Command shouldFilter={false} label="Search parts" className="quick-action-search">
              <Command.Input
                autoFocus
                value={partQuery}
                onValueChange={setPartQuery}
                placeholder="Search parts…"
                className="quick-action-search-input"
              />
              <Command.List className="quick-action-search-list">
                <Command.Empty className="quick-action-search-empty">
                  {partQuery.trim() ? 'No parts match.' : 'Type to search a part…'}
                </Command.Empty>
                {(partsSearch.data ?? []).map((hit) => (
                  <Command.Item
                    key={hit.part_id}
                    value={hit.part_id}
                    onSelect={() => pickPart(hit)}
                    className="quick-action-search-item"
                  >
                    <span className="quick-action-search-item-name">{hit.display_name}</span>
                    <span className="quick-action-search-item-meta">
                      {hit.category_name}
                      {hit.bin_label ? ` · ${hit.bin_label}` : ''}
                    </span>
                  </Command.Item>
                ))}
              </Command.List>
            </Command>
          ) : (
            <form className="quick-action-form" onSubmit={handleSubmit}>
              <NumberField
                label="Quantity"
                value={quantity}
                onChange={setQuantity}
                min={0}
                step={0.001}
                required
                autoFocus
                disabled={applyOp.isPending}
              />
              {remainingAfterText ? (
                <p className="quick-action-preview">{remainingAfterText}</p>
              ) : null}

              {config.project !== 'none' ? (
                creatingProject ? (
                  <div className="quick-action-create-project">
                    <TextField
                      label="New project name"
                      value={newProjectName}
                      onChange={setNewProjectName}
                      autoFocus
                      onKeyDown={(event) => {
                        if (event.key === 'Enter') {
                          event.preventDefault();
                          submitNewProject();
                        }
                      }}
                    />
                    <div className="quick-action-create-project-buttons">
                      <button
                        type="button"
                        className="quick-action-secondary"
                        onClick={() => {
                          setCreatingProject(false);
                          setNewProjectName('');
                        }}
                      >
                        Cancel
                      </button>
                      <button
                        type="button"
                        className="quick-action-secondary"
                        onClick={submitNewProject}
                        disabled={!newProjectName.trim() || createProject.isPending}
                      >
                        {createProject.isPending ? 'Creating…' : 'Create'}
                      </button>
                    </div>
                  </div>
                ) : (
                  <SelectField
                    label="Project"
                    value={projectId ?? ''}
                    onChange={handleProjectSelectChange}
                    options={projectSelectOptions(projects, config.project === 'required')}
                    disabled={applyOp.isPending}
                  />
                )
              ) : null}

              {request.kind === 'consume_available' ? (
                <TextField
                  label="Note"
                  value={note}
                  onChange={setNote}
                  placeholder="Optional — why it left the shelf"
                  disabled={applyOp.isPending}
                />
              ) : null}

              {request.kind === 'receive' ? (
                <div className="quick-action-details">
                  <button
                    type="button"
                    className="quick-action-details-toggle"
                    aria-expanded={showDetails}
                    onClick={() => setShowDetails((v) => !v)}
                  >
                    {showDetails ? 'Hide details' : 'Add details'}
                  </button>
                  {showDetails ? (
                    <div className="quick-action-details-fields">
                      <TextField
                        label="Supplier"
                        value={supplier}
                        onChange={setSupplier}
                        placeholder="Optional"
                        disabled={applyOp.isPending}
                      />
                      <TextField
                        label="Order"
                        value={order}
                        onChange={setOrder}
                        placeholder="Optional"
                        disabled={applyOp.isPending}
                      />
                      <TextField
                        label="Date"
                        value={date}
                        onChange={setDate}
                        placeholder="Optional"
                        disabled={applyOp.isPending}
                      />
                      <TextField
                        label="Cost"
                        value={cost}
                        onChange={setCost}
                        placeholder="Optional"
                        disabled={applyOp.isPending}
                      />
                      <TextField
                        label="Note"
                        value={note}
                        onChange={setNote}
                        placeholder="Optional"
                        disabled={applyOp.isPending}
                      />
                    </div>
                  ) : null}
                </div>
              ) : null}

              {submitError ? (
                <p className="quick-action-error">
                  {errorHint(submitError.code) ?? submitError.message}
                </p>
              ) : null}

              <div className="quick-action-buttons">
                <button
                  type="button"
                  className="quick-action-cancel"
                  onClick={onClose}
                  disabled={applyOp.isPending}
                >
                  Cancel
                </button>
                <button type="submit" className="quick-action-submit" disabled={submitDisabled}>
                  {applyOp.isPending ? 'Saving…' : config.submitLabel}
                </button>
              </div>
            </form>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
