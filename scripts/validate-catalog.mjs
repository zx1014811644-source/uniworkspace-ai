import { readFileSync } from "node:fs";

const catalog = JSON.parse(readFileSync(new URL("../catalog/apps.json", import.meta.url), "utf8"));

const webIds = [
  "chatgpt",
  "claude",
  "gemini",
  "kimi",
  "doubao",
  "workbuddy",
  "accio-work",
  "cursor",
  "deepseek",
  "harness",
];

const nativeIds = [
  "chatgpt-desktop",
  "claude-desktop",
  "lm-studio",
  "gemini-desktop",
  "kimi-desktop",
  "doubao-desktop",
  "workbuddy-desktop",
  "cursor-desktop",
  "harness-desktop",
  "accio-work-desktop",
];

const webStatuses = new Set(["official", "pending", "desktopOnly"]);
const bundleStatuses = new Set(["confirmed", "pending", "not-applicable"]);
const errors = [];

function fail(message) {
  errors.push(message);
}

if (catalog.schemaVersion !== 1) fail("schemaVersion must be 1");
if (catalog.platform !== "macos") fail("platform must be macos");
if (catalog.minimumSystemVersion !== "13.0") fail("minimumSystemVersion must be 13.0");
if (!Array.isArray(catalog.apps)) fail("apps must be an array");

const ids = new Set();
for (const app of catalog.apps ?? []) {
  if (ids.has(app.id)) fail(`duplicate id ${app.id}`);
  ids.add(app.id);
  for (const field of [
    "id",
    "name",
    "kind",
    "webUrl",
    "webStatus",
    "bundleId",
    "bundleIdStatus",
    "processNames",
    "notes",
  ]) {
    if (!(field in app)) fail(`${app.id} missing ${field}`);
  }
  if (app.kind !== "web" && app.kind !== "native") fail(`${app.id} kind`);
  if (!webStatuses.has(app.webStatus)) fail(`${app.id} webStatus`);
  if (!bundleStatuses.has(app.bundleIdStatus)) fail(`${app.id} bundleIdStatus`);
  if (!Array.isArray(app.processNames)) fail(`${app.id} processNames`);
  if (typeof app.notes !== "string" || app.notes.length < 8) fail(`${app.id} notes`);

  if (app.webStatus === "official") {
    if (typeof app.webUrl !== "string" || !app.webUrl.startsWith("https://")) {
      fail(`${app.id} official webUrl`);
    }
  } else if (app.webUrl !== null) {
    fail(`${app.id} webUrl must be null unless official`);
  }

  if (app.bundleIdStatus === "confirmed") {
    if (typeof app.bundleId !== "string" || !app.bundleId.includes(".")) {
      fail(`${app.id} confirmed bundleId`);
    }
  } else if (app.bundleId !== null) {
    fail(`${app.id} bundleId must be null unless confirmed`);
  }
}

for (const id of webIds) {
  const app = catalog.apps.find((entry) => entry.id === id);
  if (!app) fail(`missing web app ${id}`);
  else if (app.kind !== "web") fail(`${id} must be kind web`);
}

for (const id of nativeIds) {
  const app = catalog.apps.find((entry) => entry.id === id);
  if (!app) fail(`missing native app ${id}`);
  else if (app.kind !== "native") fail(`${id} must be kind native`);
}

if (catalog.apps.length !== webIds.length + nativeIds.length) {
  fail("unexpected catalog size");
}

if (errors.length > 0) {
  console.error(errors.join("\n"));
  process.exit(1);
}

console.log(`catalog ok: ${catalog.apps.length} apps`);
