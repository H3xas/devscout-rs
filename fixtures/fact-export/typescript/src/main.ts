import { target } from './target.js';
import { target as missing } from './target';
import { CartStore } from './target.js';

export function run(): number {
  const store = new CartStore();
  missing();
  return target() + store.total();
}
