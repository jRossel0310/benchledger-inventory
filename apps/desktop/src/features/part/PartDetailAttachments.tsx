/**
 * Part detail's Attachments tab (Phase 3 Task 10, spec §9): the files linked
 * to a part — datasheets, invoices, photos, drawings — stored in the
 * content-addressed backend store (identical bytes deduplicate to one file).
 *
 * Add is drag-drop or file-picker; each dropped file's bytes are read in the
 * webview and sent to `add_attachment` + `attach_to_part` (via
 * `useAddPartAttachment`). Images render a thumbnail loaded through
 * `read_attachment` -> a blob URL; other kinds show a file glyph and an Open/
 * download link. Remove unlinks the blob from this part (the shared file
 * itself is left intact for any other reference).
 */

import { useRef, useState, useEffect } from 'react';

import type { AttachmentKind, AttachmentRef, PartId } from '../../bindings.gen';
import {
  useAddPartAttachment,
  useAttachmentBytes,
  usePartAttachments,
  useRemovePartAttachment,
} from '../../hooks/inventory';
import { errorMessage, formatTimestamp } from '../../lib/format';
import './PartDetailAttachments.css';

export interface PartDetailAttachmentsProps {
  partId: PartId;
}

const KIND_LABELS: Record<AttachmentKind, string> = {
  invoice: 'Invoice',
  datasheet: 'Datasheet',
  photo: 'Photo',
  measurement_photo: 'Measurement photo',
  drawing: 'Drawing',
  cad: 'CAD',
  project_doc: 'Project doc',
  other: 'File',
};

const IMAGE_EXTS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'avif', 'ico']);

const MIME_BY_EXT: Record<string, string> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  svg: 'image/svg+xml',
  bmp: 'image/bmp',
  avif: 'image/avif',
  ico: 'image/x-icon',
  pdf: 'application/pdf',
  txt: 'text/plain',
  csv: 'text/csv',
  json: 'application/json',
};

/** Whether an attachment should render as an image thumbnail: an image file
 * extension, or (when the extension is unknown) a photo-like kind. */
function isImageAttachment(a: AttachmentRef): boolean {
  if (a.ext && IMAGE_EXTS.has(a.ext.toLowerCase())) return true;
  return a.kind === 'photo' || a.kind === 'measurement_photo';
}

function mimeForExt(ext: string | null): string {
  if (!ext) return 'application/octet-stream';
  return MIME_BY_EXT[ext.toLowerCase()] ?? 'application/octet-stream';
}

/** The file's extension (lowercase, no dot), or `null` when the name has none. */
function extFromName(name: string): string | null {
  const dot = name.lastIndexOf('.');
  if (dot <= 0 || dot === name.length - 1) return null;
  return name.slice(dot + 1).toLowerCase();
}

/** A best-effort attachment kind from a picked file's MIME type — images are
 * photos, PDFs default to datasheet (the most common PDF a part carries),
 * everything else is a generic file. Purely a default; the backend accepts any
 * kind and the value is not load-bearing. */
function kindForFile(file: File): AttachmentKind {
  if (file.type.startsWith('image/')) return 'photo';
  if (file.type === 'application/pdf') return 'datasheet';
  return 'other';
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

/** Fetch a blob's bytes and expose them as an object URL, or `undefined` while
 * loading and `null` when disabled/failed. Revokes the URL on change/unmount. */
function useObjectUrl(attachment: AttachmentRef, enabled: boolean): string | null | undefined {
  const bytesQuery = useAttachmentBytes(attachment.content_hash, enabled);
  const [url, setUrl] = useState<string | null | undefined>(enabled ? undefined : null);

  useEffect(() => {
    if (!enabled || !bytesQuery.data) return;
    const blob = new Blob([new Uint8Array(bytesQuery.data)], { type: mimeForExt(attachment.ext) });
    const objectUrl = URL.createObjectURL(blob);
    setUrl(objectUrl);
    return () => URL.revokeObjectURL(objectUrl);
  }, [enabled, bytesQuery.data, attachment.ext]);

  if (bytesQuery.isError) return null;
  return url;
}

export function PartDetailAttachments({ partId }: PartDetailAttachmentsProps) {
  const attachmentsQuery = usePartAttachments(partId);
  const inputRef = useRef<HTMLInputElement>(null);
  const [dragOver, setDragOver] = useState(false);
  const [addError, setAddError] = useState<string | null>(null);

  const addMutation = useAddPartAttachment({
    onDone: (error) => setAddError(error ? errorMessage(error) : null),
  });

  async function handleFiles(files: FileList | File[]) {
    setAddError(null);
    for (const file of Array.from(files)) {
      const buffer = await file.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buffer));
      addMutation.mutate({
        partId,
        bytes,
        ext: extFromName(file.name),
        kind: kindForFile(file),
        originalName: file.name,
        source: 'upload',
      });
    }
  }

  return (
    <div className="attachments">
      <div
        className={`attachments-dropzone${dragOver ? ' attachments-dropzone-active' : ''}`}
        onDragOver={(event) => {
          event.preventDefault();
          setDragOver(true);
        }}
        onDragLeave={() => setDragOver(false)}
        onDrop={(event) => {
          event.preventDefault();
          setDragOver(false);
          if (event.dataTransfer.files.length > 0) {
            void handleFiles(event.dataTransfer.files);
          }
        }}
      >
        <p className="attachments-dropzone-hint">Drag a file here to attach it, or</p>
        <button
          type="button"
          className="part-detail-action-secondary"
          onClick={() => inputRef.current?.click()}
          disabled={addMutation.isPending}
        >
          {addMutation.isPending ? 'Uploading…' : 'Choose file'}
        </button>
        <input
          ref={inputRef}
          type="file"
          className="attachments-file-input"
          aria-label="Attach a file"
          onChange={(event) => {
            if (event.target.files && event.target.files.length > 0) {
              void handleFiles(event.target.files);
            }
            // Reset so re-picking the same file still fires onChange.
            event.target.value = '';
          }}
        />
      </div>

      {addError ? (
        <p className="part-detail-status part-detail-status-error">
          Could not attach file: {addError}
        </p>
      ) : null}

      {attachmentsQuery.isPending ? (
        <p className="part-detail-status">Loading attachments…</p>
      ) : attachmentsQuery.isError ? (
        <p className="part-detail-status part-detail-status-error">
          Could not load attachments: {errorMessage(attachmentsQuery.error)}
        </p>
      ) : (attachmentsQuery.data ?? []).length === 0 ? (
        <p className="part-detail-status">No attachments yet — drag a file above to add one.</p>
      ) : (
        <ul className="attachments-list">
          {(attachmentsQuery.data ?? []).map((attachment) => (
            <AttachmentRow key={attachment.content_hash} partId={partId} attachment={attachment} />
          ))}
        </ul>
      )}
    </div>
  );
}

interface AttachmentRowProps {
  partId: PartId;
  attachment: AttachmentRef;
}

function AttachmentRow({ partId, attachment }: AttachmentRowProps) {
  const removeMutation = useRemovePartAttachment();
  const url = useObjectUrl(attachment, true);
  const isImage = isImageAttachment(attachment);
  const name = attachment.original_name ?? attachment.content_hash;

  return (
    <li className="attachments-row">
      {isImage ? (
        <AttachmentThumbnail url={url} name={name} />
      ) : (
        <span className="attachments-glyph" aria-hidden>
          {(attachment.ext ?? 'file').slice(0, 4).toUpperCase()}
        </span>
      )}
      <div className="attachments-meta">
        <span className="attachments-name">{name}</span>
        <span className="attachments-sub part-detail-mono">
          {KIND_LABELS[attachment.kind]} · {formatBytes(attachment.size_bytes)} ·{' '}
          {formatTimestamp(attachment.created_at)}
        </span>
      </div>
      <div className="attachments-actions">
        {url ? (
          <a
            className="part-detail-link-button"
            href={url}
            download={name}
            target="_blank"
            rel="noreferrer"
          >
            Open
          </a>
        ) : null}
        <button
          type="button"
          className="part-detail-link-button"
          onClick={() => removeMutation.mutate({ partId, contentHash: attachment.content_hash })}
          disabled={removeMutation.isPending}
        >
          Remove
        </button>
      </div>
    </li>
  );
}

interface AttachmentThumbnailProps {
  url: string | null | undefined;
  name: string;
}

function AttachmentThumbnail({ url, name }: AttachmentThumbnailProps) {
  if (url === undefined) {
    return <span className="attachments-thumb attachments-thumb-loading" aria-hidden />;
  }
  if (url === null) {
    return (
      <span className="attachments-glyph" aria-hidden>
        IMG
      </span>
    );
  }
  return (
    <a href={url} target="_blank" rel="noreferrer" className="attachments-thumb-link">
      <img className="attachments-thumb" src={url} alt={name} />
    </a>
  );
}
