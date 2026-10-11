const { total } = require('../../money.mjs');

function jsDeepTotal() {
  return total([1]);
}

module.exports = { jsDeepTotal };
