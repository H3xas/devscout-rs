const { total } = require('./money.mjs');

function checksTotal() {
  return total([1, 2]);
}

module.exports = { checksTotal };
