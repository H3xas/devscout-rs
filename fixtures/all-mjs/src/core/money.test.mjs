import { test } from 'node:test';
import assert from 'node:assert';
import { total, Wallet } from './money.mjs';

test('total counts its items', () => {
  assert.equal(total([1, 2]), 2);
});

test('a wallet starts empty', () => {
  assert.equal(new Wallet().balance(), 0);
});
