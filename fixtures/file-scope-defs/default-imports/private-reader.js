import { spinWheel } from './private-barrel';
import { spinWheel as piped } from './spinner';
import * as parts from './spinner';

const direct = require('./spinner');

export function peek() {
  spinWheel();
  piped();
  parts.spinWheel();
  direct.spinWheel();
}
