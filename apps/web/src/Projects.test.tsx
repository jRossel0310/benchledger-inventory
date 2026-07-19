import { parseSnapshot, type Snapshot } from '@ei/shared';
import { cleanup, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import sample from '../../../packages/shared/fixtures/sample-snapshot.json';
import { Projects } from './Projects';

const parsed = parseSnapshot(sample);
if (parsed === null) throw new Error('sample-snapshot.json failed to parse');
const snapshot: Snapshot = parsed;

afterEach(cleanup);

describe('Projects', () => {
  it('renders every project with its status chip in the status token class', () => {
    render(<Projects snapshot={snapshot} />);
    expect(screen.getByText('Bench PSU Rebuild')).toBeTruthy();
    expect(screen.getByText('Blinky Board')).toBeTruthy();
    const active = screen.getByText('Active');
    expect(active.className).toContain('project-status-active');
    const planned = screen.getByText('Planned');
    expect(planned.className).toContain('project-status-planned');
  });

  it('falls back to a neutral chip for an unknown status', () => {
    const first = snapshot.projects[0];
    if (first === undefined) throw new Error('sample snapshot has no projects');
    const odd: Snapshot = {
      ...snapshot,
      projects: [{ ...first, status: 'someday' }],
    };
    render(<Projects snapshot={odd} />);
    const chip = screen.getByText('Someday');
    expect(chip.className).toContain('project-status-unknown');
  });

  it('shows description and build quantity when present', () => {
    render(<Projects snapshot={snapshot} />);
    expect(screen.getByText(/ATmega328P LED blinker/)).toBeTruthy();
    const blinky = screen.getByText('Blinky Board').closest('.project-card') as HTMLElement;
    expect(within(blinky).getByText('Build quantity:', { exact: false })).toBeTruthy();
    expect(within(blinky).getByText('5')).toBeTruthy();
  });

  it('links each associated part to its detail route', () => {
    render(<Projects snapshot={snapshot} />);
    const blinky = screen.getByText('Blinky Board').closest('.project-card') as HTMLElement;
    const links = within(blinky).getAllByRole('link');
    expect(links.length).toBe(7);
    const resistor = within(blinky).getByRole('link', { name: '10k 0603 1% resistor' });
    expect(resistor.getAttribute('href')).toBe('#/part/ID000000000000000000000005');
  });

  it('says so when a project has no associated parts', () => {
    render(<Projects snapshot={snapshot} />);
    const psu = screen.getByText('Bench PSU Rebuild').closest('.project-card') as HTMLElement;
    expect(within(psu).getByText('No parts associated.')).toBeTruthy();
  });
});
