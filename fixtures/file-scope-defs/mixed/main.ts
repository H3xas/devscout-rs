export function publicTs(): number { return localTs(); }
function localTs(): number { return 4; }
interface LocalShape { value: number }
type LocalAlias = LocalShape;
