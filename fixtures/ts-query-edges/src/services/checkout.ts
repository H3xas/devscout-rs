import * as actions from '../actions/cartActions';
import { CartStore as Store } from '../store/cartStore';

export function checkout() {
  actions.clearCart();
  return new Store();
}
