export function walkTree(depth: number): number {
  return depth > 0 ? walkTree(depth - 1) : 0;
}
