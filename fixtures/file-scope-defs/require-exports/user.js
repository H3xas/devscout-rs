const w = require('./whole');
const { impl } = require('./prop');
function go() { w.run(); impl.run(); }
module.exports = { go };
