export function total(items) {
  return items.length;
}

export class Wallet {
  constructor() {
    this.items = [];
  }

  balance() {
    return total(this.items);
  }
}
