export function truncate(value: string, maxLength: number): string {
  return value.length > maxLength ? `${value.slice(0, maxLength - 1)}...` : value;
}

export function slugify(value: string): string {
  return value.trim().toLowerCase().replace(/\s+/g, '-');
}

export const DEFAULT_MAX_LENGTH = 80;
