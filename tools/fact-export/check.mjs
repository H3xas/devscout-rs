#!/usr/bin/env node
// The export contract's second reader: a dependency-free check that streams a
// pinned export line by line, validates every line against the committed JSON
// Schema, and recomputes the trailer's payload digest from the fact bytes.
//
// It exists because the Rust suite validates the same contract with the same
// code that produced it. A round trip through another language, reading the
// schema as a document rather than as a Rust type, is what makes "the schema
// describes the stream" a claim rather than a restatement.
//
// It holds one line at a time and hashes exact fact bytes as they arrive, so
// the largest export this repository knows how to produce costs no more memory
// than its longest line.

import { createHash } from "node:crypto";
import { createReadStream, existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..", "..");
const SCHEMA_PATH = join(ROOT, "docs", "reference", "fact-export.schema.json");
const PIN_DIR = join(ROOT, "fixtures", "fact-export");
const NEGATIVE_DIR = join(HERE, "negative");

const FORMAT = "devscout-facts";

// Every keyword the schema may use. A schema that reaches for one outside this
// set is refused rather than partly applied: a check that silently ignored a
// constraint would pass everything it did not understand.
const SUPPORTED_KEYWORDS = new Set([
  "$schema",
  "$id",
  "$defs",
  "$ref",
  "title",
  "description",
  "type",
  "const",
  "enum",
  "required",
  "properties",
  "additionalProperties",
  "items",
  "oneOf",
  "pattern",
  "minimum",
]);

const FAMILIES = [
  "definition",
  "edge",
  "publish-site",
  "handler-registration",
  "di-registration",
  "unresolved-import",
  "test-file",
  "transport-address",
];

// One named defect per negative fixture, so the selftest asserts WHY a stream
// was refused rather than only that it was.
const NEGATIVES = {
  "undeclared-key.ndjson": "is not a declared key",
  "non-integer-line.ndjson": "line is a number",
  "malformed-line.ndjson": "is not JSON",
  "missing-trailer.ndjson": "the last line is not a trailer",
  "digest-mismatch.ndjson": "payloadDigest does not match the fact lines",
};

function fail(message) {
  throw new Error(message);
}

function assertSubset(node, where) {
  if (node === null || typeof node !== "object") return;
  if (Array.isArray(node)) {
    node.forEach((entry, index) => assertSubset(entry, `${where}[${index}]`));
    return;
  }
  for (const [key, value] of Object.entries(node)) {
    if (!SUPPORTED_KEYWORDS.has(key)) {
      fail(`${where}: the schema uses \`${key}\`, which is outside the supported subset`);
    }
    // `$defs` and `properties` map NAMES to schemas: the names are the
    // document's own vocabulary, and only their values are schemas.
    if (["$defs", "properties"].includes(key)) {
      for (const [name, sub] of Object.entries(value)) {
        assertSubset(sub, `${where}.${key}.${name}`);
      }
    }
    if (["items", "oneOf", "additionalProperties"].includes(key)) {
      assertSubset(value, `${where}.${key}`);
    }
  }
}

function typeMatches(value, type) {
  switch (type) {
    case "object":
      return value !== null && typeof value === "object" && !Array.isArray(value);
    case "array":
      return Array.isArray(value);
    case "string":
      return typeof value === "string";
    case "boolean":
      return typeof value === "boolean";
    case "null":
      return value === null;
    case "integer":
      return Number.isInteger(value);
    case "number":
      return typeof value === "number";
    default:
      return false;
  }
}

function validate(value, schema, root, where) {
  if (schema.$ref !== undefined) {
    const prefix = "#/$defs/";
    if (!schema.$ref.startsWith(prefix)) {
      fail(`${where}: \`${schema.$ref}\` is not a local $defs reference`);
    }
    const name = schema.$ref.slice(prefix.length);
    const target = root.$defs?.[name];
    if (target === undefined) fail(`${where}: \`${schema.$ref}\` names no definition`);
    return validate(value, target, root, where);
  }
  const shape = schema.type;
  const types = Array.isArray(shape) ? shape : shape === undefined ? [] : [shape];
  if (types.length && !types.some((type) => typeMatches(value, type))) {
    fail(`${where}: line is a ${Array.isArray(value) ? "array" : typeof value}, expected ${types.join(" or ")}`);
  }
  if (schema.const !== undefined && JSON.stringify(value) !== JSON.stringify(schema.const)) {
    fail(`${where}: expected the constant ${JSON.stringify(schema.const)}`);
  }
  if (schema.enum !== undefined && !schema.enum.some((allowed) => JSON.stringify(allowed) === JSON.stringify(value))) {
    fail(`${where}: ${JSON.stringify(value)} is not one of the enumerated values`);
  }
  if (schema.minimum !== undefined && typeof value === "number" && value < schema.minimum) {
    fail(`${where}: ${value} is below the minimum ${schema.minimum}`);
  }
  if (schema.pattern !== undefined) {
    if (typeof value !== "string") fail(`${where}: a pattern applies to a string`);
    if (!new RegExp(schema.pattern).test(value)) {
      fail(`${where}: ${value} does not match ${schema.pattern}`);
    }
  }
  if (schema.required !== undefined) {
    for (const name of schema.required) {
      if (!Object.hasOwn(value, name)) fail(`${where}: the required key \`${name}\` is missing`);
    }
  }
  if (schema.properties !== undefined) {
    for (const [name, sub] of Object.entries(value)) {
      const property = schema.properties[name];
      if (property !== undefined) {
        validate(sub, property, root, `${where}.${name}`);
      } else if (schema.additionalProperties === false) {
        fail(`${where}: \`${name}\` is not a declared key`);
      }
    }
  }
  if (schema.items !== undefined) {
    value.forEach((item, index) => validate(item, schema.items, root, `${where}[${index}]`));
  }
  if (schema.oneOf !== undefined) {
    const failures = [];
    let matched = 0;
    for (const option of schema.oneOf) {
      try {
        validate(value, option, root, where);
        matched += 1;
      } catch (error) {
        failures.push({ option, message: String(error.message) });
      }
    }
    if (matched !== 1) {
      fail(
        `${where}: the line matches ${matched} of the oneOf branches, and wants exactly one: ${closestFailure(value, failures, root)}`,
      );
    }
  }
}

// The branch that declares the most of the line's own keys is the one the
// line was trying to be, so its error is the one worth quoting: reporting only
// the count would hide the key or the type that actually went wrong.
function closestFailure(value, failures, root) {
  let best = "";
  let bestScore = -1;
  let bestDeclared = Number.POSITIVE_INFINITY;
  const keys = value !== null && typeof value === "object" ? Object.keys(value) : [];
  for (const { option, message } of failures) {
    const declared = declaredKeys(option, root);
    const score = keys.filter((key) => declared.has(key)).length;
    // Ties go to the narrower branch: a branch declaring every key of every
    // family is not a better fit than one declaring exactly this line's keys.
    if (score > bestScore || (score === bestScore && declared.size < bestDeclared)) {
      bestScore = score;
      bestDeclared = declared.size;
      best = message;
    }
  }
  return best;
}

function declaredKeys(schema, root) {
  if (schema.$ref !== undefined) {
    const name = schema.$ref.slice("#/$defs/".length);
    return declaredKeys(root.$defs?.[name] ?? {}, root);
  }
  const keys = new Set(Object.keys(schema.properties ?? {}));
  for (const option of schema.oneOf ?? []) {
    for (const key of declaredKeys(option, root)) keys.add(key);
  }
  return keys;
}

function parseLine(line, where) {
  try {
    return JSON.parse(line);
  } catch {
    fail(`${where}: is not JSON`);
  }
}

async function checkStream(path, schema) {
  const hash = createHash("sha256");
  const counts = Object.fromEntries(FAMILIES.map((family) => [family, 0]));
  let header = null;
  let deferred = null;
  let facts = 0;
  let lines = 0;
  let sawCarriageReturn = false;
  let pending = "";

  const flushFact = (line, where) => {
    const value = parseLine(line, where);
    validate(value, schema, schema, where);
    if (!FAMILIES.includes(value.family)) fail(`${where}: \`${value.family}\` is not a family`);
    counts[value.family] += 1;
    facts += 1;
    // The digest covers the fact lines' exact bytes, each with its newline.
    hash.update(`${line}\n`, "utf8");
  };

  const accept = (line, where) => {
    if (header === null) {
      header = parseLine(line, where);
      validate(header, schema, schema, where);
      if (header.format !== FORMAT) fail(`${where}: the header is not a \`${FORMAT}\` header`);
      return;
    }
    if (deferred !== null) flushFact(deferred.line, deferred.where);
    deferred = { line, where };
  };

  const stream = createReadStream(path, { encoding: "utf8" });
  for await (const chunk of stream) {
    if (chunk.includes("\r")) sawCarriageReturn = true;
    pending += chunk;
    let index = pending.indexOf("\n");
    while (index >= 0) {
      const line = pending.slice(0, index);
      pending = pending.slice(index + 1);
      lines += 1;
      accept(line, `line ${lines}`);
      index = pending.indexOf("\n");
    }
  }
  if (sawCarriageReturn) fail("the stream carries a carriage return");
  if (pending.length) fail("the stream does not end with a newline");
  if (header === null) fail("the stream has no header");
  if (deferred === null) fail("the stream does not end with a trailer");
  if (lines < 2) fail("the stream has no trailer line");

  const trailer = parseLine(deferred.line, deferred.where);
  validate(trailer, schema, schema, deferred.where);
  if (trailer.trailer !== FORMAT) fail(`${deferred.where}: the last line is not a trailer`);
  if (trailer.facts !== facts) {
    fail(`the trailer counts ${trailer.facts} facts and the stream carries ${facts}`);
  }
  for (const family of FAMILIES) {
    if (trailer.counts[family] !== counts[family]) {
      fail(`the trailer counts ${trailer.counts[family]} ${family} lines and the stream carries ${counts[family]}`);
    }
  }
  const digest = `sha256:${hash.digest("hex")}`;
  if (trailer.payloadDigest !== digest) {
    fail(`the trailer's payloadDigest does not match the fact lines: ${trailer.payloadDigest} against ${digest}`);
  }
  return { facts, counts };
}

function checkFile(path, schema) {
  return checkStream(path, schema);
}

async function selftest(schema) {
  const pins = readdirSync(PIN_DIR)
    .filter((name) => name.endsWith(".ndjson"))
    .sort();
  if (!pins.length) fail("no pinned export to check");
  for (const name of pins) {
    const summary = await checkFile(join(PIN_DIR, name), schema);
    process.stdout.write(`${name}: ok (${summary.facts} facts)\n`);
  }
  let refused = 0;
  for (const [name, reason] of Object.entries(NEGATIVES)) {
    const path = join(NEGATIVE_DIR, name);
    if (!existsSync(path)) fail(`${name} is named as a negative fixture and is not on disk`);
    try {
      await checkFile(path, schema);
    } catch (error) {
      if (!String(error.message).includes(reason)) {
        fail(`${name}: refused for the wrong reason: ${error.message}`);
      }
      refused += 1;
      continue;
    }
    fail(`${name}: was accepted and must be refused`);
  }
  process.stdout.write(`negatives: ${refused} refused, each for its own reason\n`);
}

async function main(argv) {
  const schema = JSON.parse(readFileSync(SCHEMA_PATH, "utf8"));
  assertSubset(schema, "schema");
  if (argv.includes("--selftest")) {
    await selftest(schema);
    return 0;
  }
  if (!argv.length) {
    process.stderr.write("usage: node tools/fact-export/check.mjs <export.ndjson> ... | --selftest\n");
    return 2;
  }
  for (const path of argv) {
    const summary = await checkFile(path, schema);
    process.stdout.write(`${path}: ok (${summary.facts} facts)\n`);
  }
  return 0;
}

main(process.argv.slice(2)).then(
  (code) => {
    process.exitCode = code;
  },
  (error) => {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  },
);
