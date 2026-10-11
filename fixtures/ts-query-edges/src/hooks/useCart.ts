import { CartStore } from '../store/cartStore';
import type { CartItem } from '../model';

export function useCart(): CartItem[] {
  const store = new CartStore();
  store.total();
  return [];
}
