const { loadCart } = require('../actions/cartActions');

function run() {
  return loadCart();
}

module.exports = { run };
