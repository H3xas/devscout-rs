const path = require('path');

function joinSafe(base, ...segments) {
  return path.join(base, ...segments).replace(/\\/g, '/');
}

function isAbsolute(candidate) {
  return path.isAbsolute(candidate);
}

module.exports = {
  joinSafe,
  isAbsolute,
};
