export interface CartItem {
  id: string;
  qty: number;
}

export type CartId = string;

export default function emptyCart(): CartItem[] {
  return [];
}
