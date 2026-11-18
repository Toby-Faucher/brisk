import { describe, expect, it } from 'vitest';
import { pick, type Box } from './padnav';

const box = (x: number, y: number): Box => ({ left: x, top: y, right: x + 10, bottom: y + 10 });

describe('pick', () => {
  const grid = [box(0, 0), box(50, 0), box(0, 50), box(50, 50)];
  it('pick_moves_along_rows_and_columns', () => {
    expect(pick(grid[0], 'right', grid)).toBe(1);
    expect(pick(grid[0], 'down', grid)).toBe(2);
    expect(pick(grid[3], 'left', grid)).toBe(2);
    expect(pick(grid[3], 'up', grid)).toBe(1);
  });
  it('pick_returns_minus_one_at_the_edge', () => {
    expect(pick(grid[0], 'left', grid)).toBe(-1);
    expect(pick(grid[0], 'up', grid)).toBe(-1);
  });
  it('pick_prefers_same_row_over_nearer_diagonal', () => {
    expect(pick(box(0, 0), 'right', [box(20, 30), box(60, 0)])).toBe(1);
  });
});
