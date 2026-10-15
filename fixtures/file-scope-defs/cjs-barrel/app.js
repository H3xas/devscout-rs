const { foo, bar } = require('./lib/index');
function main() { foo.foo(); bar(); }
module.exports = { main };
