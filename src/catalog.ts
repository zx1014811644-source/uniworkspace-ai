import raw from "../catalog/apps.json";
import type { Catalog, CatalogApp } from "./types";

export const catalog = raw as Catalog;

export function appsByKind(kind: CatalogApp["kind"]): CatalogApp[] {
  return catalog.apps.filter((app) => app.kind === kind);
}
