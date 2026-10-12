import { useCart } from '../hooks/useCart';

export function CartBadge() {
  const items = useCart();
  return <span>{items.length}</span>;
}
