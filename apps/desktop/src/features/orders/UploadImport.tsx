/**
 * Upload -> Extract (Phase 5d Task 2, spec §10): a file picker + drag-drop
 * zone for a DigiKey order confirmation/invoice (PDF, CSV, or XLSX). Reads
 * the dropped/picked file's bytes in the webview and calls `useParseImport`
 * (`parse_and_store_import`) — the same "read `File.arrayBuffer()` into a
 * plain byte array" convention `PartDetailAttachments.tsx` uses for
 * `add_attachment`, since both cross the IPC boundary as a JSON number
 * array (`Vec<u8>` -> `number[]`).
 *
 * The backend detects the format from the filename/bytes and only persists
 * an `ImportRecord` once parsing succeeds (`parse_and_store_import_impl`:
 * `detect_format`/`parse_invoice` both `?`-propagate before `store_import`
 * runs) — so any error here means nothing was stored, and the copy below
 * says so rather than leaving the user unsure whether a half-parsed import
 * is sitting in the list. Duplicate-order detection is NOT done here: it
 * surfaces after parsing, on the review screen (`ImportReview.duplicate_
 * of`) — this component never blocks an upload on it.
 *
 * On success, navigates straight to the new import's review route (Task 3
 * fills in the real screen; until then `ImportReviewPage`'s placeholder
 * shell confirms the import was found) — the same "create, then land on the
 * thing you created" flow `ProjectsList`'s create form uses.
 */

import { useNavigate } from '@tanstack/react-router';
import { useRef, useState } from 'react';

import { useToast } from '../../components/Toast';
import { useParseImport } from '../../hooks/imports';
import { errorMessage } from '../../lib/format';
import './UploadImport.css';

export function UploadImport() {
  const navigate = useNavigate();
  const { toast } = useToast();
  const inputRef = useRef<HTMLInputElement>(null);
  const [dragOver, setDragOver] = useState(false);
  const [uploadError, setUploadError] = useState<string | null>(null);

  const parseImport = useParseImport({
    onDone: (error, data) => {
      if (error) {
        setUploadError(errorMessage(error));
        return;
      }
      if (data) {
        setUploadError(null);
        toast({ title: `Imported ${data.supplier} order`, kind: 'success' });
        void navigate({ to: '/orders/$importId', params: { importId: data.id } });
      }
    },
  });

  async function handleFile(file: File) {
    setUploadError(null);
    const buffer = await file.arrayBuffer();
    const bytes = Array.from(new Uint8Array(buffer));
    parseImport.mutate({ bytes, filename: file.name });
  }

  function handleFiles(files: FileList | File[]) {
    // One order at a time — a multi-file drop only uploads the first entry
    // rather than silently queueing the rest.
    const file = Array.from(files)[0];
    if (!file) return;
    void handleFile(file);
  }

  return (
    <div
      className={`upload-import${dragOver ? ' upload-import-active' : ''}`}
      onDragOver={(event) => {
        event.preventDefault();
        setDragOver(true);
      }}
      onDragLeave={() => setDragOver(false)}
      onDrop={(event) => {
        event.preventDefault();
        setDragOver(false);
        if (event.dataTransfer.files.length > 0) handleFiles(event.dataTransfer.files);
      }}
    >
      <p className="upload-import-hint">
        Drag a DigiKey order confirmation or invoice here (PDF, CSV, or XLSX), or
      </p>
      <button
        type="button"
        className="upload-import-button"
        onClick={() => inputRef.current?.click()}
        disabled={parseImport.isPending}
      >
        {parseImport.isPending ? 'Uploading…' : 'Choose file'}
      </button>
      <input
        ref={inputRef}
        type="file"
        accept=".pdf,.csv,.xlsx"
        className="upload-import-file-input"
        aria-label="Upload order file"
        onChange={(event) => {
          if (event.target.files && event.target.files.length > 0) {
            handleFiles(event.target.files);
          }
          // Reset so re-picking the same file still fires onChange.
          event.target.value = '';
        }}
      />
      {uploadError ? (
        <p className="upload-import-error">
          Could not read this file: {uploadError} Nothing was stored — pick a different file or try
          again.
        </p>
      ) : null}
    </div>
  );
}
