export default (amount: number, currencyCode: string = 'USD') =>
  new Intl.NumberFormat('en-US', { style: 'currency', currency: currencyCode }).format(amount);
