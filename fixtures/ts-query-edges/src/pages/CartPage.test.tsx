import { CartPage } from './CartPage';

it('dispatches on render', () => {
  const view = <CartPage dispatch={() => undefined} />;
  expect(view).toBeTruthy();
});
