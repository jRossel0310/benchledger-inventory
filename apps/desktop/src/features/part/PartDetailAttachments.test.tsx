import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      listPartAttachments: vi.fn(),
      addAttachment: vi.fn(),
      attachToPart: vi.fn(),
      readAttachment: vi.fn(),
      removePartAttachment: vi.fn(),
    },
  };
});

import type { AttachmentRef } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { PartDetailAttachments } from './PartDetailAttachments';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function attachment(overrides: Partial<AttachmentRef> = {}): AttachmentRef {
  return {
    content_hash: 'hash-abc',
    ext: 'pdf',
    size_bytes: 2048,
    kind: 'datasheet',
    original_name: 'lm358.pdf',
    source: 'upload',
    created_at: '2026-07-15 10:00:00',
    ...overrides,
  };
}

beforeEach(() => {
  vi.resetAllMocks();
  // jsdom implements neither of these; the component builds/revokes blob URLs.
  globalThis.URL.createObjectURL = vi.fn(() => 'blob:mock-url');
  globalThis.URL.revokeObjectURL = vi.fn();
  // Default: reading any blob's bytes succeeds. Only image rows fetch this
  // eagerly on render — non-image rows only fetch once Open is clicked.
  vi.mocked(commands.readAttachment).mockReturnValue(ok([1, 2, 3]));
});

afterEach(cleanup);

function renderAttachments() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <PartDetailAttachments partId="p1" />
    </QueryClientProvider>,
  );
}

describe('PartDetailAttachments', () => {
  it('renders the list of attachments from the command', async () => {
    vi.mocked(commands.listPartAttachments).mockReturnValue(ok([attachment()]));

    renderAttachments();

    await waitFor(() => expect(screen.getByText('lm358.pdf')).toBeTruthy());
    expect(screen.getByText(/Datasheet/)).toBeTruthy();
    expect(screen.getByText(/2\.0 KB/)).toBeTruthy();
  });

  it('shows an empty state when the part has no attachments', async () => {
    vi.mocked(commands.listPartAttachments).mockReturnValue(ok([]));

    renderAttachments();

    await waitFor(() => expect(screen.getByText(/no attachments yet/i)).toBeTruthy());
  });

  it('renders an image attachment as a thumbnail loaded from read_attachment', async () => {
    vi.mocked(commands.listPartAttachments).mockReturnValue(
      ok([
        attachment({
          content_hash: 'img-1',
          ext: 'png',
          kind: 'photo',
          original_name: 'front.png',
        }),
      ]),
    );

    renderAttachments();

    const img = await screen.findByRole('img', { name: 'front.png' });
    expect(img.getAttribute('src')).toBe('blob:mock-url');
    expect(commands.readAttachment).toHaveBeenCalledWith('img-1');
  });

  it('does not fetch a non-image attachment on render, only when Open is clicked', async () => {
    vi.mocked(commands.listPartAttachments).mockReturnValue(ok([attachment()]));

    renderAttachments();

    await waitFor(() => expect(screen.getByText('lm358.pdf')).toBeTruthy());
    // A non-image row's bytes must not have been fetched just by rendering
    // the tab — that would defeat the point of deferring the read.
    expect(commands.readAttachment).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Open' }));

    await waitFor(() => expect(commands.readAttachment).toHaveBeenCalledWith('hash-abc'));
    expect(commands.readAttachment).toHaveBeenCalledTimes(1);
  });

  it('reads the dropped file bytes and calls add_attachment then attach_to_part', async () => {
    vi.mocked(commands.listPartAttachments).mockReturnValue(ok([]));
    vi.mocked(commands.addAttachment).mockReturnValue(ok(attachment({ content_hash: 'new-hash' })));
    vi.mocked(commands.attachToPart).mockReturnValue(ok(null));

    renderAttachments();
    await waitFor(() => expect(screen.getByText(/no attachments yet/i)).toBeTruthy());

    const bytes = new Uint8Array([10, 20, 30]);
    const file = new File([bytes], 'chip.png', { type: 'image/png' });
    // jsdom's File does not implement arrayBuffer(); the real webview File
    // does. Polyfill it so handleFiles can read the bytes.
    Object.defineProperty(file, 'arrayBuffer', { value: () => Promise.resolve(bytes.buffer) });
    const input = screen.getByLabelText('Attach a file') as HTMLInputElement;
    fireEvent.change(input, { target: { files: [file] } });

    await waitFor(() => expect(commands.addAttachment).toHaveBeenCalledTimes(1));
    expect(commands.addAttachment).toHaveBeenCalledWith(
      [10, 20, 30],
      'png',
      'photo',
      'chip.png',
      'upload',
    );
    await waitFor(() => expect(commands.attachToPart).toHaveBeenCalledWith('p1', 'new-hash'));
  });

  it('awaits each file in a multi-file drop sequentially rather than firing them all at once', async () => {
    vi.mocked(commands.listPartAttachments).mockReturnValue(ok([]));
    vi.mocked(commands.attachToPart).mockReturnValue(ok(null));

    // Gate the first add so it doesn't resolve until we say so, and prove
    // the second file's add_attachment call doesn't fire until it does.
    let resolveFirst: (() => void) | undefined;
    const firstGate = new Promise<void>((resolve) => {
      resolveFirst = resolve;
    });
    vi.mocked(commands.addAttachment).mockImplementation(async (bytes) => {
      const hash = bytes[0] === 1 ? 'hash-1' : 'hash-2';
      if (hash === 'hash-1') await firstGate;
      return { status: 'ok' as const, data: attachment({ content_hash: hash }) };
    });

    renderAttachments();
    await waitFor(() => expect(screen.getByText(/no attachments yet/i)).toBeTruthy());

    const fileA = new File([new Uint8Array([1])], 'a.pdf', { type: 'application/pdf' });
    const fileB = new File([new Uint8Array([2])], 'b.pdf', { type: 'application/pdf' });
    for (const [file, byte] of [
      [fileA, 1],
      [fileB, 2],
    ] as const) {
      Object.defineProperty(file, 'arrayBuffer', {
        value: () => Promise.resolve(new Uint8Array([byte]).buffer),
      });
    }

    const input = screen.getByLabelText('Attach a file') as HTMLInputElement;
    fireEvent.change(input, { target: { files: [fileA, fileB] } });

    await waitFor(() => expect(commands.addAttachment).toHaveBeenCalledTimes(1));
    // Give any stray microtasks a chance to run, then confirm the second
    // file's add still hasn't started while the first is gated open.
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(commands.addAttachment).toHaveBeenCalledTimes(1);

    resolveFirst?.();

    await waitFor(() => expect(commands.addAttachment).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(commands.attachToPart).toHaveBeenCalledTimes(2));
  });

  it('removes a part attachment link when Remove is clicked', async () => {
    vi.mocked(commands.listPartAttachments).mockReturnValue(ok([attachment()]));
    vi.mocked(commands.removePartAttachment).mockReturnValue(ok(null));

    renderAttachments();
    await waitFor(() => expect(screen.getByText('lm358.pdf')).toBeTruthy());

    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));

    await waitFor(() =>
      expect(commands.removePartAttachment).toHaveBeenCalledWith('p1', 'hash-abc'),
    );
  });
});
