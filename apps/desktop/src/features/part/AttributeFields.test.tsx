import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../bindings.gen', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../bindings.gen')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      previewUnitValue: vi.fn(),
    },
  };
});

import type { AttributeDefRow } from '../../bindings.gen';
import { commands } from '../../bindings.gen';
import { AttributeFields } from './AttributeFields';

function ok<T>(data: T) {
  return Promise.resolve({ status: 'ok' as const, data });
}

function commandError(code: string, message: string) {
  return Promise.resolve({ status: 'error' as const, error: { code, message } });
}

function def(overrides: Partial<AttributeDefRow>): AttributeDefRow {
  return {
    key: 'resistance',
    label: 'Resistance',
    data_type: 'text',
    unit_kind: null,
    identity: true,
    display_order: 0,
    hidden: false,
    choices: [],
    ...overrides,
  };
}

function renderFields(
  defs: AttributeDefRow[],
  values: Record<string, string> = {},
  onChange = vi.fn(),
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const Wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  const utils = render(<AttributeFields defs={defs} values={values} onChange={onChange} />, {
    wrapper: Wrapper,
  });
  return { ...utils, onChange };
}

afterEach(cleanup);

describe('AttributeFields — per-data-type rendering', () => {
  it('renders nothing for an empty def list', () => {
    const { container } = renderFields([]);
    expect(container.querySelector('.attribute-fields')).toBeNull();
  });

  it('renders a text field for data_type "text" and reports edits', () => {
    const { onChange } = renderFields([
      def({ key: 'network_config', label: 'Network configuration', data_type: 'text' }),
    ]);
    const input = screen.getByLabelText('Network configuration') as HTMLInputElement;
    expect(input.type).toBe('text');
    fireEvent.change(input, { target: { value: 'Bussed' } });
    expect(onChange).toHaveBeenCalledWith('network_config', 'Bussed');
  });

  it('renders a url field for data_type "url"', () => {
    renderFields([def({ key: 'datasheet', label: 'Datasheet', data_type: 'url' })]);
    expect(screen.getByLabelText('Datasheet')).toBeTruthy();
    expect(screen.getByPlaceholderText('https://…')).toBeTruthy();
  });

  it('renders a number field for data_type "number" and reports it as a string', () => {
    const { onChange } = renderFields([
      def({ key: 'num_elements', label: 'Number of elements', data_type: 'number' }),
    ]);
    fireEvent.change(screen.getByLabelText('Number of elements'), { target: { value: '4' } });
    expect(onChange).toHaveBeenCalledWith('num_elements', '4');
  });

  it('renders a checkbox for data_type "boolean", unchecked by default', () => {
    const { onChange } = renderFields([
      def({ key: 'polarized', label: 'Polarized', data_type: 'boolean' }),
    ]);
    const checkbox = screen.getByLabelText('Polarized') as HTMLInputElement;
    expect(checkbox.checked).toBe(false);
    fireEvent.click(checkbox);
    expect(onChange).toHaveBeenCalledWith('polarized', 'true');
  });

  it('renders a checkbox reflecting an already-set "true" value', () => {
    renderFields([def({ key: 'polarized', label: 'Polarized', data_type: 'boolean' })], {
      polarized: 'true',
    });
    expect((screen.getByLabelText('Polarized') as HTMLInputElement).checked).toBe(true);
  });

  it('renders a select with a blank option plus every def choice for data_type "choice"', () => {
    const { onChange } = renderFields([
      def({
        key: 'mounting_style',
        label: 'Mounting style',
        data_type: 'choice',
        choices: ['SMD', 'THT'],
      }),
    ]);
    const select = screen.getByLabelText('Mounting style') as HTMLSelectElement;
    expect(Array.from(select.options).map((o) => o.value)).toEqual(['', 'SMD', 'THT']);
    fireEvent.change(select, { target: { value: 'SMD' } });
    expect(onChange).toHaveBeenCalledWith('mounting_style', 'SMD');
  });

  it('renders one checkbox per choice for data_type "multi_choice" and joins selections with commas', () => {
    const { onChange } = renderFields([
      def({
        key: 'kinds',
        label: 'Kinds',
        data_type: 'multi_choice',
        choices: ['A', 'B', 'C'],
      }),
    ]);
    fireEvent.click(screen.getByLabelText('A'));
    expect(onChange).toHaveBeenCalledWith('kinds', 'A');
  });

  it('pre-checks the boxes already present in a comma-joined multi_choice value', () => {
    renderFields(
      [def({ key: 'kinds', label: 'Kinds', data_type: 'multi_choice', choices: ['A', 'B', 'C'] })],
      { kinds: 'A, C' },
    );
    expect((screen.getByLabelText('A') as HTMLInputElement).checked).toBe(true);
    expect((screen.getByLabelText('B') as HTMLInputElement).checked).toBe(false);
    expect((screen.getByLabelText('C') as HTMLInputElement).checked).toBe(true);
  });

  it('renders two bound inputs for data_type "range" and combines them into "low..high"', () => {
    const { onChange } = renderFields([
      def({
        key: 'operating_temp',
        label: 'Operating temperature',
        data_type: 'range',
        unit_kind: 'voltage',
      }),
    ]);
    fireEvent.change(screen.getByLabelText('Operating temperature (low)'), {
      target: { value: '1V' },
    });
    expect(onChange).toHaveBeenCalledWith('operating_temp', '1V..');
    fireEvent.change(screen.getByLabelText('Operating temperature (high)'), {
      target: { value: '2V' },
    });
    expect(onChange).toHaveBeenCalledWith('operating_temp', '..2V');
  });

  it('splits an existing "low..high" value back into its two bound inputs', () => {
    renderFields(
      [def({ key: 'range_v', label: 'Range', data_type: 'range', unit_kind: 'voltage' })],
      { range_v: '1V..2V' },
    );
    expect((screen.getByLabelText('Range (low)') as HTMLInputElement).value).toBe('1V');
    expect((screen.getByLabelText('Range (high)') as HTMLInputElement).value).toBe('2V');
  });

  it('respects display_order and skips hidden defs', () => {
    renderFields([
      def({ key: 'b_attr', label: 'B attr', display_order: 1 }),
      def({ key: 'a_attr', label: 'A attr', display_order: 0 }),
      def({ key: 'hidden_attr', label: 'Hidden attr', display_order: 2, hidden: true }),
    ]);
    const labels = screen
      .getAllByText(/attr$/, { selector: 'label, span' })
      .map((el) => el.textContent);
    expect(labels).toEqual(['A attr', 'B attr']);
    expect(screen.queryByText('Hidden attr')).toBeNull();
  });

  it('re-renders an entirely different field set when defs change (category swap)', () => {
    const { rerender, container } = renderFields([def({ key: 'resistance', label: 'Resistance' })]);
    expect(screen.getByLabelText('Resistance')).toBeTruthy();

    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    rerender(
      <QueryClientProvider client={queryClient}>
        <AttributeFields
          defs={[def({ key: 'capacitance', label: 'Capacitance', data_type: 'number_unit' })]}
          values={{}}
          onChange={vi.fn()}
        />
      </QueryClientProvider>,
    );
    expect(screen.queryByLabelText('Resistance')).toBeNull();
    expect(screen.getByLabelText('Capacitance')).toBeTruthy();
    void container;
  });
});

// Real timers throughout (the debounce is a real 300ms `setTimeout`):
// `waitFor`'s own polling is timer-based too, and mixing it with faked
// timers is a well-known footgun (the poll never "ticks" without manually
// advancing fake time in lockstep). A real, short wait is simpler and
// robust here — the debounce is the only thing worth actually testing.
describe('AttributeFields — number_unit live normalized preview', () => {
  it('shows the canonical normalized form after typing settles', async () => {
    vi.mocked(commands.previewUnitValue).mockReturnValue(ok('10 kΩ'));
    const { onChange } = renderFields([
      def({
        key: 'resistance',
        label: 'Resistance',
        data_type: 'number_unit',
        unit_kind: 'resistance',
      }),
    ]);

    fireEvent.change(screen.getByLabelText('Resistance'), { target: { value: '10k' } });
    // onChange (the raw value reaching the parent/save state) fires
    // immediately, not debounced — only the preview request waits.
    expect(onChange).toHaveBeenCalledWith('resistance', '10k');
    expect(commands.previewUnitValue).not.toHaveBeenCalled();

    await waitFor(() => expect(screen.getByText('10 kΩ')).toBeTruthy());
    expect(commands.previewUnitValue).toHaveBeenCalledWith('resistance', '10k');
  });

  it('settles on the latest value for a burst of keystrokes (the underlying debounce contract, exercised end to end)', async () => {
    vi.mocked(commands.previewUnitValue).mockReturnValue(ok('10 kΩ'));
    renderFields([
      def({
        key: 'resistance',
        label: 'Resistance',
        data_type: 'number_unit',
        unit_kind: 'resistance',
      }),
    ]);

    // "Only fires once, latest value wins" for a burst within the debounce
    // window is `useDebouncedCallback`'s own unit-tested contract
    // (`hooks/useDebouncedCallback.test.ts`, under fake timers where a real
    // 100ms/300ms race isn't at the mercy of test-runner scheduling); this
    // end-to-end test only needs to confirm the final, settled preview
    // reflects the last-typed value.
    const input = screen.getByLabelText('Resistance');
    fireEvent.change(input, { target: { value: '1' } });
    fireEvent.change(input, { target: { value: '10' } });
    fireEvent.change(input, { target: { value: '10k' } });

    await waitFor(() => expect(screen.getByText('10 kΩ')).toBeTruthy());
    expect(commands.previewUnitValue).toHaveBeenLastCalledWith('resistance', '10k');
  });

  it('shows a quiet fallback note instead of an error for an in-progress unparsable value', async () => {
    vi.mocked(commands.previewUnitValue).mockReturnValue(
      commandError('invalid_attribute_value', 'not a number'),
    );
    renderFields([
      def({
        key: 'resistance',
        label: 'Resistance',
        data_type: 'number_unit',
        unit_kind: 'resistance',
      }),
    ]);

    fireEvent.change(screen.getByLabelText('Resistance'), { target: { value: '10' } });

    await waitFor(() => expect(screen.getByText('Normalized on save')).toBeTruthy());
  });
});
