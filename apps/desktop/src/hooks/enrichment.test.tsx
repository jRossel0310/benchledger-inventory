import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { DigiKeyStatus, EnrichmentDiff } from '../bindings.gen';
import { commands } from '../lib/commands';
import {
  keys,
  useApplyEnrichment,
  useDigiKeyStatus,
  useEnrichmentPreview,
  useSetDigiKeyEnvironment,
} from './enrichment';

function makeClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
}

function wrapperFor(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('query keys', () => {
  it('extend inventory.ts’s keys with the enrichment entries', () => {
    // The shared entity keys (spread from inventory.ts) are still present...
    expect(keys.allParts).toEqual(['parts']);
    expect(keys.dashboardSummary).toEqual(['dashboardSummary']);
    // ...alongside the new enrichment keys.
    expect(keys.enrichmentPreview('p1')).toEqual(['enrichmentPreview', 'p1']);
    expect(keys.digikeyStatus).toEqual(['digikeyStatus']);
  });
});

describe('useEnrichmentPreview', () => {
  it('is disabled by default (lazy) and does not call enrich_part_preview on mount', async () => {
    const spy = vi
      .spyOn(commands, 'enrichPartPreview')
      .mockResolvedValue({ status: 'ok', data: {} as EnrichmentDiff });
    const queryClient = makeClient();

    renderHook(() => useEnrichmentPreview('p1'), { wrapper: wrapperFor(queryClient) });

    // Give any accidental fetch a chance to fire before asserting it didn't.
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(spy).not.toHaveBeenCalled();
  });

  it('stays disabled without a partId even when enabled: true is requested', async () => {
    const spy = vi
      .spyOn(commands, 'enrichPartPreview')
      .mockResolvedValue({ status: 'ok', data: {} as EnrichmentDiff });
    const queryClient = makeClient();

    renderHook(() => useEnrichmentPreview(undefined, { enabled: true }), {
      wrapper: wrapperFor(queryClient),
    });

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(spy).not.toHaveBeenCalled();
  });

  it('fetches once a partId is present AND enabled: true is passed', async () => {
    const diff = { part_id: 'p1', diffs: [], notes: [], provider_summary: [] } as EnrichmentDiff;
    vi.spyOn(commands, 'enrichPartPreview').mockResolvedValue({ status: 'ok', data: diff });
    const queryClient = makeClient();

    const { result } = renderHook(() => useEnrichmentPreview('p1', { enabled: true }), {
      wrapper: wrapperFor(queryClient),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(commands.enrichPartPreview).toHaveBeenCalledWith('p1');
    expect(result.current.data).toEqual(diff);
  });
});

describe('useDigiKeyStatus', () => {
  it('calls commands.getDigikeyStatus', async () => {
    const status = { configured: false, environment: 'sandbox' } as DigiKeyStatus;
    vi.spyOn(commands, 'getDigikeyStatus').mockResolvedValue({ status: 'ok', data: status });
    const queryClient = makeClient();

    const { result } = renderHook(() => useDigiKeyStatus(), { wrapper: wrapperFor(queryClient) });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(commands.getDigikeyStatus).toHaveBeenCalled();
    expect(result.current.data).toEqual(status);
  });
});

describe('useApplyEnrichment', () => {
  it('calls applyEnrichment with partId/applied and invalidates the expected key set', async () => {
    vi.spyOn(commands, 'applyEnrichment').mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useApplyEnrichment(), { wrapper: wrapperFor(queryClient) });
    const applied = [
      { key: 'variant.datasheet_url', value: 'https://x/ds.pdf', source: 'digikey' },
    ];
    await act(async () => {
      await result.current.mutateAsync({ partId: 'p1', applied });
    });

    expect(commands.applyEnrichment).toHaveBeenCalledWith('p1', applied);

    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.part('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.attributes('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.variants('p1') }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allParts }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.allSearch }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.dashboardSummary }),
    );
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.enrichmentPreview('p1') }),
    );
  });
});

describe('useSetDigiKeyEnvironment', () => {
  it('calls setDigikeyEnvironment and invalidates digikeyStatus', async () => {
    vi.spyOn(commands, 'setDigikeyEnvironment').mockResolvedValue({ status: 'ok', data: null });
    const queryClient = makeClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(() => useSetDigiKeyEnvironment(), {
      wrapper: wrapperFor(queryClient),
    });
    await act(async () => {
      await result.current.mutateAsync('production');
    });

    expect(commands.setDigikeyEnvironment).toHaveBeenCalledWith('production');
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: keys.digikeyStatus }),
    );
  });

  it('surfaces a CommandError (e.g. an invalid environment string) as a rejected mutation', async () => {
    const error = { code: 'invalid_digikey_environment', message: 'bad env' };
    vi.spyOn(commands, 'setDigikeyEnvironment').mockResolvedValue({ status: 'error', error });
    const queryClient = makeClient();

    const { result } = renderHook(() => useSetDigiKeyEnvironment(), {
      wrapper: wrapperFor(queryClient),
    });
    await act(async () => {
      await expect(result.current.mutateAsync('prod')).rejects.toEqual(error);
    });
  });
});
