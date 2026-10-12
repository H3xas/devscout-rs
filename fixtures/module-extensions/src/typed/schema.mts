export interface Row {
  id: string;
}

export function parseRow(raw: string): Row {
  return { id: raw };
}
