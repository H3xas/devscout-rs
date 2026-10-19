import { crankHandle } from './barrel-default';
import dialReading from './barrel-bare';

export function assemble() {
  return crankHandle() + dialReading();
}
