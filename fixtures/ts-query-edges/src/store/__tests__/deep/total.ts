import { CartStore } from '../../cartStore';

it('totals zero', () => {
  expect(new CartStore().total()).toBe(0);
});
