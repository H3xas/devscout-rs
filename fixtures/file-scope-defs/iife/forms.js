function programFirst() {}
(function() { function normal() {} function programFirst() {} const boundInside = require('./other'); })();
(function() { function insideCall() {} }());
(() => { function arrowed() {} })();
(async function() { function asyncNormal() {} })();
(async () => { function asyncArrow() {} })();
NS.part = (function() { function assigned() {} })();
var Holder = (function() { function initialised() {} })();
(function() { (function() { function nested() {} })(); })();
function ordinary() { function hiddenInFunction() {} }
if (true) { function hiddenInBlock() {} }
class Outer { method() { function hiddenInMethod() {} } }
(function() { function hiddenInCall() {} }).call(this);
!function() { function hiddenInUnary() {} }();
consume((function() { function hiddenInArgument() {} })());
