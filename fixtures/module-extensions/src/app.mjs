import { total } from './core/money.mjs';
import { Wallet } from './core';
import { formatAmount } from './util/format';
import { tool } from './build/tool';
import { parseRow } from './typed/schema.mts';

export function summarize(items) {
  return formatAmount(total(items));
}

export function makeWallet() {
  return new Wallet();
}

export function runTool() {
  return tool();
}

export function readRow(raw) {
  return parseRow(raw);
}
