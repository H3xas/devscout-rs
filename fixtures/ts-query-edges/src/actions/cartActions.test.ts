import { loadCart } from './cartActions';

it('loads', () => {
  expect(loadCart().type).toBe('load');
});
