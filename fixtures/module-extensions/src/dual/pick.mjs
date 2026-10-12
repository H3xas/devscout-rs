import { thing } from './thing.mjs';
import { emitted } from './emitted.mjs';
import { flagged } from './flagged.cjs';

export function picked() {
  return `${thing()}${emitted()}${flagged()}`;
}
