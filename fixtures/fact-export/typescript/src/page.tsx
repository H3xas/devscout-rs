import { CartBadge } from './badge.js';
import { loadCart } from './actions.js';

declare const dispatch: (action: unknown) => void;

export function CartPage(): unknown {
  dispatch(loadCart());
  return <CartBadge />;
}
