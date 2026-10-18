function plain() {}
function* generated() { yield 1; }
class PlainClass {}
abstract class AbstractClass {}
interface PlainInterface { x: number }
type PlainType = string;
enum PlainEnum { A }
const alpha = 1, { beta, gamma: renamed } = { beta: 2, gamma: 3 };
let delta = 4;
var epsilon = 5;
const imported = require('./other'), kept = 6;
const dynamic = require(path);
