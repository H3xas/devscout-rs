function localAction() {}
class LocalWidget {}
function LocalView() { return <LocalWidget />; }
function host() { localAction(); new LocalWidget(); dispatch(localAction); }
class Caller { run() { localAction(); } }
(() => { localAction(); })();
const localObject = { ping() {} };
localObject.ping();
