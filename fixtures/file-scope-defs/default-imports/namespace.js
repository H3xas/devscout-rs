import * as parts from './spinner';
import * as dials from './dial';

export function turn() {
  return parts.default() + dials.default();
}
