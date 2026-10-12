const { total } = require('../../money.mjs');

function cjsDeepTotal() {
  return total([1, 2]);
}

module.exports = { cjsDeepTotal };
