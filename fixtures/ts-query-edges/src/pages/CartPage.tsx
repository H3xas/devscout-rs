import { CartBadge } from '../components/CartBadge';
import { loadCart } from '../actions/cartActions';
import emptyCart from '../model/cart';
import { type CartId, CartItem } from '../model/cart';

export function CartPage({ dispatch }: { dispatch: (action: unknown) => void }) {
  dispatch(loadCart());
  const empty: CartItem[] = emptyCart();
  const id: CartId = 'main';
  return <CartBadge key={id} data-size={empty.length} />;
}
