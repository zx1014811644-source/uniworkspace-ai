export type AppKind = "web" | "native";
export type WebStatus = "official" | "pending" | "desktopOnly";
export type BundleIdStatus = "confirmed" | "pending" | "not-applicable";

export interface CatalogApp {
  id: string;
  name: string;
  kind: AppKind;
  webUrl: string | null;
  webStatus: WebStatus;
  bundleId: string | null;
  bundleIdStatus: BundleIdStatus;
  processNames: string[];
  notes: string;
}

export interface Catalog {
  schemaVersion: 1;
  platform: "macos";
  minimumSystemVersion: "13.0";
  researchedAt: string;
  apps: CatalogApp[];
}
