import { describe, expect, it } from 'vitest';
import { errMsg } from './errMsg';

describe('errMsg', () => {
  it('uses an Error message', () => expect(errMsg(new TypeError('boom'))).toBe('boom'));
  it('shows a non-Error rejection as text', () => {
    expect(errMsg('offline')).toBe('offline');
    expect(errMsg(42)).toBe('42');
  });
});
