import { total } from './core/money.mjs';
import { Wallet } from './core';
import { formatAmount } from './util/format';

export function summarize(items) {
  return formatAmount(total(items));
}

export function makeWallet() {
  return new Wallet();
}
