import spinWheel from './spinner';
import dialReading from './dial';

export function readBoth() {
  return spinWheel() + dialReading();
}
