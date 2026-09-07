export async function placeOrder(items) {
  return fetch("/checkout/place-order", {
    method: "POST",
    body: JSON.stringify(items),
  });
}
