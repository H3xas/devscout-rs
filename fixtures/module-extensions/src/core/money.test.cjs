const { total } = require('./money.mjs');

function checksEmpty() {
  return total([]);
}

module.exports = { checksEmpty };
