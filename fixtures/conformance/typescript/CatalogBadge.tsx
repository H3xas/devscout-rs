import { CatalogItem } from "./catalogTypes";

export interface CatalogBadgeProps {
  item: CatalogItem;
}

export function CatalogBadge({ item }: CatalogBadgeProps) {
  return <span>{item.label}</span>;
}
