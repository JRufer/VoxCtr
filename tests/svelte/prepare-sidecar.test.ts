import { describe, it, expect } from 'vitest';
import { selectBinary } from '../../scripts/prepare-sidecar.mjs';

describe('prepare-sidecar selectBinary helper', () => {
  it('selects the only existing binary candidate', () => {
    const candidates = ['path/to/release/binary', 'path/to/debug/binary'];
    const existsSync = (p: string) => p === 'path/to/debug/binary';
    const statSync = (p: string) => ({ mtimeMs: 1000 } as any);

    const result = selectBinary(candidates, existsSync, statSync);
    expect(result).toBe('path/to/debug/binary');
  });

  it('selects the newest binary candidate when multiple exist', () => {
    const candidates = ['path/to/release/binary', 'path/to/debug/binary'];
    const existsSync = (p: string) => true; // both exist
    const statSync = (p: string) => {
      if (p === 'path/to/release/binary') {
        return { mtimeMs: 1000 } as any;
      }
      return { mtimeMs: 2000 } as any; // debug is newer
    };

    const result = selectBinary(candidates, existsSync, statSync);
    expect(result).toBe('path/to/debug/binary');
  });

  it('throws an error if no candidates exist', () => {
    const candidates = ['path/to/release/binary', 'path/to/debug/binary'];
    const existsSync = (p: string) => false;
    const statSync = (p: string) => ({ mtimeMs: 1000 } as any);

    expect(() => selectBinary(candidates, existsSync, statSync)).toThrowError(
      'Overlay binary not found'
    );
  });
});
