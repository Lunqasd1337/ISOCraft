import fs from "node:fs";
import path from "node:path";

const ROOT = process.cwd();
const RU_PATH = path.join(ROOT, "src/i18n/locales/ru.json");
const EN_PATH = path.join(ROOT, "src/i18n/locales/en.json");
const SRC_DIR = path.join(ROOT, "src");

const PLURAL_SUFFIX_RE = /_(zero|one|two|few|many|other)$/;
const RU_REQUIRED_PLURALS = ["one", "few", "many"];
const EN_REQUIRED_PLURALS = ["one", "other"];
const DYNAMIC_BASE_PREFIXES = [
  "screens.build.missingKinds.",
  "screens.build.steps.",
];

function flatten(obj, localeLabel, prefix = "") {
  const out = {};
  for (const [key, val] of Object.entries(obj)) {
    const fullKey = prefix ? `${prefix}.${key}` : key;
    if (val !== null && typeof val === "object" && !Array.isArray(val)) {
      Object.assign(out, flatten(val, localeLabel, fullKey));
    } else if (typeof val === "string") {
      out[fullKey] = val;
    } else {
      throw new Error(
        `Invalid non-string translation value in ${localeLabel} at "${fullKey}" (type: ${Array.isArray(val) ? "array" : typeof val})`
      );
    }
  }
  return out;
}

function splitPlural(fullKey) {
  const match = fullKey.match(PLURAL_SUFFIX_RE);
  if (!match) return { base: fullKey, form: null };
  return {
    base: fullKey.slice(0, -match[0].length),
    form: match[1],
  };
}

function extractPlaceholders(text) {
  const found = new Set();
  for (const m of text.matchAll(/\{\{(\w+)\}\}/g)) {
    found.add(m[1]);
  }
  return [...found].sort();
}

function walkSourceFiles(dir) {
  const results = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      results.push(...walkSourceFiles(full));
    } else if (/\.(ts|tsx)$/.test(entry.name) && !entry.name.endsWith(".d.ts")) {
      results.push(full);
    }
  }
  return results;
}

function readLocaleJson(filePath, label) {
  try {
    return JSON.parse(fs.readFileSync(filePath, "utf8"));
  } catch (err) {
    console.error(`Failed reading/parsing ${label} (${filePath}): ${err.message}`);
    process.exit(1);
  }
}

const ru = readLocaleJson(RU_PATH, "ru.json");
const en = readLocaleJson(EN_PATH, "en.json");

const ruFlat = flatten(ru, "ru.json");
const enFlat = flatten(en, "en.json");

function groupByBase(flatMap) {
  const groups = new Map();
  for (const [fullKey, value] of Object.entries(flatMap)) {
    const { base, form } = splitPlural(fullKey);
    if (!groups.has(base)) {
      groups.set(base, { forms: new Map() });
    }
    groups.get(base).forms.set(form ?? "__plain__", value);
  }
  return groups;
}

const ruGroups = groupByBase(ruFlat);
const enGroups = groupByBase(enFlat);

const errors = [];

// 1. Base-key parity
for (const base of ruGroups.keys()) {
  if (!enGroups.has(base)) {
    errors.push(`Key present only in ru.json: "${base}"`);
  }
}
for (const base of enGroups.keys()) {
  if (!ruGroups.has(base)) {
    errors.push(`Key present only in en.json: "${base}"`);
  }
}

// 2. Plural completeness & placeholder parity
const allBases = new Set([...ruGroups.keys(), ...enGroups.keys()]);
for (const base of allBases) {
  const ruEntry = ruGroups.get(base);
  const enEntry = enGroups.get(base);
  if (!ruEntry || !enEntry) continue;

  const ruHasPlural = [...ruEntry.forms.keys()].some((k) => k !== "__plain__");
  const enHasPlural = [...enEntry.forms.keys()].some((k) => k !== "__plain__");

  if (ruHasPlural !== enHasPlural) {
    errors.push(`Pluralization mismatch for "${base}" between ru.json and en.json`);
    continue;
  }

  if (ruHasPlural) {
    if (ruEntry.forms.has("__plain__")) {
      errors.push(`ru.json has both plain and plural forms for "${base}"`);
    }
    if (enEntry.forms.has("__plain__")) {
      errors.push(`en.json has both plain and plural forms for "${base}"`);
    }
    for (const req of RU_REQUIRED_PLURALS) {
      if (!ruEntry.forms.has(req)) {
        errors.push(`ru.json missing plural form "${base}_${req}"`);
      }
    }
    for (const req of EN_REQUIRED_PLURALS) {
      if (!enEntry.forms.has(req)) {
        errors.push(`en.json missing plural form "${base}_${req}"`);
      }
    }
  }

  const refPlaceholders = extractPlaceholders([...enEntry.forms.values()][0]);
  for (const [form, val] of ruEntry.forms.entries()) {
    const p = extractPlaceholders(val);
    if (JSON.stringify(p) !== JSON.stringify(refPlaceholders)) {
      errors.push(
        `Placeholder mismatch in ru.json "${base}${form === "__plain__" ? "" : `_${form}`}": expected [${refPlaceholders.join(", ")}], got [${p.join(", ")}]`
      );
    }
  }
  for (const [form, val] of enEntry.forms.entries()) {
    const p = extractPlaceholders(val);
    if (JSON.stringify(p) !== JSON.stringify(refPlaceholders)) {
      errors.push(
        `Placeholder mismatch in en.json "${base}${form === "__plain__" ? "" : `_${form}`}": expected [${refPlaceholders.join(", ")}], got [${p.join(", ")}]`
      );
    }
  }
}

// 3. Scan src/**/*.{ts,tsx} for used and missing keys
const sourceFiles = walkSourceFiles(SRC_DIR);
const staticCallKeys = new Set();
const allLiteralKeys = new Set();

for (const file of sourceFiles) {
  const content = fs.readFileSync(file, "utf8");
  for (const m of content.matchAll(/\bt\(\s*["']([^"']+)["']/g)) {
    staticCallKeys.add(m[1]);
  }
  for (const m of content.matchAll(/["']([a-zA-Z0-9_]+(?:\.[a-zA-Z0-9_]+)+)["']/g)) {
    allLiteralKeys.add(m[1]);
  }
}

for (const key of staticCallKeys) {
  if (!ruGroups.has(key) || !enGroups.has(key)) {
    errors.push(`Missing translation key referenced in source: "${key}"`);
  }
}

const unusedBases = [];
for (const base of allBases) {
  const isUsedLiterally = staticCallKeys.has(base) || allLiteralKeys.has(base);
  const isDynamic = DYNAMIC_BASE_PREFIXES.some((prefix) => base.startsWith(prefix));
  if (!isUsedLiterally && !isDynamic) {
    unusedBases.push(base);
  }
}

if (unusedBases.length > 0) {
  errors.push(`Unused translation keys (${unusedBases.length}): ${unusedBases.join(", ")}`);
}

if (errors.length > 0) {
  console.error("i18n check failed:");
  for (const err of errors) {
    console.error(`  - ${err}`);
  }
  process.exit(1);
}

console.log(
  `i18n check passed: ${allBases.size} base keys (${Object.keys(ruFlat).length} ru / ${Object.keys(enFlat).length} en entries), 0 missing, 0 unused.`
);
