export function publicFn() { return localFn(); }
function localFn() { return 7; }
class LocalClass {}
const localArrow = () => localFn();
