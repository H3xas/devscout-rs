import { useCart } from './useCart';

it('starts empty', () => {
  expect(useCart()).toEqual([]);
});
