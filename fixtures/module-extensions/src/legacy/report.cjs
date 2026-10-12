const { grandTotal } = require('./totals');

function report(items) {
  return grandTotal(items);
}

module.exports = { report };
