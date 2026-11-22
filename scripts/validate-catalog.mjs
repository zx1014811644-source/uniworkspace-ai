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

const officialUrls = {
  chatgpt: "https://chatgpt.com/",
  claude: "https://claude.ai/",
  gemini: "https://gemini.google.com/",
  kimi: "https://www.kimi.com/",
  doubao: "https://www.doubao.com/chat/",
  workbuddy: "https://www.workbuddy.cn/app",
  deepseek: "https://chat.deepseek.com/",
};

const pendingWebIds = ["accio-work", "cursor", "harness"];
const pendingBundleIds = ["workbuddy-desktop", "harness-desktop", "accio-work-desktop"];

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

for (const [id, url] of Object.entries(officialUrls)) {
  const app = catalog.apps.find((entry) => entry.id === id);
  if (!app) fail(`missing official web app ${id}`);
  else if (app.webUrl !== url) fail(`${id} webUrl must stay ${url}`);
  else if (app.webStatus !== "official") fail(`${id} must stay official`);
}

for (const id of pendingWebIds) {
  const app = catalog.apps.find((entry) => entry.id === id);
  if (!app) fail(`missing pending web app ${id}`);
  else if (app.webUrl !== null) fail(`${id} webUrl must stay null`);
  else if (app.webStatus !== "pending") fail(`${id} must stay pending`);
  else if (!String(app.notes).includes("does not open a WebView")) {
    fail(`${id} must say it does not open a WebView`);
  }
}

for (const id of pendingBundleIds) {
  const app = catalog.apps.find((entry) => entry.id === id);
  if (!app) fail(`missing ${id}`);
  else if (app.bundleId !== null) fail(`${id} bundleId must stay null`);
}

const confirmedBundles = {
  "chatgpt-desktop": "com.openai.codex",
  "claude-desktop": "com.anthropic.claudefordesktop",
  "lm-studio": "ai.elementlabs.lmstudio",
  "gemini-desktop": "com.google.GeminiMacOS",
  "kimi-desktop": "com.moonshot.kimichat",
  "doubao-desktop": "com.bot.pc.doubao",
  "cursor-desktop": "com.todesktop.230313mzl4w4u92",
};

for (const [id, bundleId] of Object.entries(confirmedBundles)) {
  const app = catalog.apps.find((entry) => entry.id === id);
  if (!app) fail(`missing ${id}`);
  else if (app.bundleId !== bundleId) fail(`${id} bundleId must stay ${bundleId}`);
  else if (app.bundleIdStatus !== "confirmed") fail(`${id} must stay confirmed`);
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

const compat = JSON.parse(
  readFileSync(new URL("../catalog/compatibility.json", import.meta.url), "utf8"),
);

if (compat.schemaVersion !== 1) fail("compatibility schemaVersion");
if (compat.nativeSnapLimit !== 1) fail("native snap limit must stay 1");
if (compat.nativeSnapCeiling !== 2) fail("native snap ceiling must stay 2");
if (compat.nativeSnapLimit > compat.nativeSnapCeiling) fail("snap limit above ceiling");
if ("webUrl" in compat || "bundleId" in compat) {
  fail("compatibility checklist must not carry urls or bundle ids");
}
if (!Array.isArray(compat.openP0)) fail("openP0 must be an array");
for (const item of compat.openP0) {
  if (typeof item !== "string" || item.length < 4) fail("openP0 entry");
  if (/https?:\/\//.test(item) || item.includes("com.") || item.includes("ai.")) {
    fail("openP0 must not invent a url or bundle id");
  }
}

function sameIds(actual, expected, label) {
  const left = [...actual].sort().join(",");
  const right = [...expected].sort().join(",");
  if (left !== right) fail(`${label} ids must be ${right}`);
}

sameIds(compat.confirmedWeb, Object.keys(officialUrls), "confirmed web");
sameIds(compat.pendingDoNotOpen, pendingWebIds, "pending web");
sameIds(compat.confirmedDesktop, Object.keys(confirmedBundles), "confirmed desktop");
sameIds(compat.pendingBundle, pendingBundleIds, "pending bundle");

for (const id of compat.confirmedWeb) {
  const app = catalog.apps.find((entry) => entry.id === id);
  if (!app || app.webUrl !== officialUrls[id]) fail(`${id} checklist url drifted`);
}
for (const id of compat.pendingDoNotOpen) {
  const app = catalog.apps.find((entry) => entry.id === id);
  if (!app || app.webUrl !== null) fail(`${id} must stay closed`);
}
for (const id of compat.confirmedDesktop) {
  const app = catalog.apps.find((entry) => entry.id === id);
  if (!app || app.bundleId !== confirmedBundles[id]) fail(`${id} checklist bundle drifted`);
}
for (const id of compat.pendingBundle) {
  const app = catalog.apps.find((entry) => entry.id === id);
  if (!app || app.bundleId !== null) fail(`${id} bundle must stay empty`);
}

if (errors.length > 0) {
  console.error(errors.join("\n"));
  process.exit(1);
}

console.log(`catalog ok: ${catalog.apps.length} apps`);
