#!/usr/bin/env node
// Resolve the module sites in a synthetic case with TypeScript's own API.
const fs = require('node:fs');
const path = require('node:path');

const [typescriptPath, casePath, fixtureName, filterPrefix, outputPath] = process.argv.slice(2);
if (!typescriptPath || !casePath) {
  throw new Error('usage: ts_resolution_truth.cjs <typescript.js> <case-directory>');
}
const ts = require(path.resolve(typescriptPath));
const root = path.resolve(casePath);
const caseName = fixtureName || path.basename(root);
const configCache = new Map();
function parsedConfig(configPath) {
  if (configCache.has(configPath)) return configCache.get(configPath);
  if (!fs.existsSync(configPath)) return undefined;
  const config = ts.readConfigFile(configPath, ts.sys.readFile);
  if (config.error) return undefined;
  const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, path.dirname(configPath));
  const options = { ...parsed.options, noDtsResolution: true };
  configCache.set(configPath, options);
  return options;
}
function configFor(file) {
  let dir = path.dirname(file);
  while (dir.startsWith(root)) {
    const options = parsedConfig(path.join(dir, 'tsconfig.json'));
    if (options && (options.moduleResolution !== undefined ||
      [ts.ModuleKind.Node16, ts.ModuleKind.NodeNext, ts.ModuleKind.Preserve].includes(options.module))) return options;
    if (dir === root) break;
    dir = path.dirname(dir);
  }
  return parsedConfig(path.join(root, 'tsconfig.base.json')) || parsedConfig(path.join(root, 'tsconfig.json'));
}
const indexed = new Set();
const candidatePackages = new Map();
function walk(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory() && !['node_modules', '.git', '.scout', 'bin', 'obj', 'dist', 'coverage', '.next', 'target'].includes(entry.name)) {
      walk(full);
    } else if (entry.isFile() && /\.(ts|tsx|js|jsx|mts|cts|mjs|cjs)$/.test(entry.name)
      && !/\.d\.(ts|mts|cts)$/.test(entry.name)) {
      indexed.add(full);
    }
  }
}
walk(root);
for (const file of [...indexed]) {
  let dir = path.dirname(file);
  while (dir.startsWith(root)) {
    const manifest = path.join(dir, 'package.json');
    if (fs.existsSync(manifest)) {
      try {
        const pkg = JSON.parse(fs.readFileSync(manifest, 'utf8').replace(/^\uFEFF/, ''));
        if (pkg.name && dir !== root) candidatePackages.set(dir, pkg.name);
      } catch { /* A template manifest is not an installed package. */ }
      break;
    }
    if (dir === root) break;
    dir = path.dirname(dir);
  }
}
function workspacePatterns() {
  const manifest = path.join(root, 'package.json');
  const pkg = fs.existsSync(manifest) ? JSON.parse(fs.readFileSync(manifest, 'utf8').replace(/^\uFEFF/, '')) : {};
  const npm = Array.isArray(pkg.workspaces) ? pkg.workspaces : pkg.workspaces?.packages;
  const yaml = path.join(root, 'pnpm-workspace.yaml');
  if (!fs.existsSync(yaml)) return npm || [];
  const lines = fs.readFileSync(yaml, 'utf8').split(/\r?\n/);
  const pnpm = [];
  let section = false;
  for (const line of lines) {
    if (line.startsWith('packages:')) { section = true; continue; }
    if (section && /^\S/.test(line) && line.trim()) section = false;
    if (section && /^\s*-\s+/.test(line)) pnpm.push(line.replace(/^\s*-\s+/, '').trim().replace(/^['"]|['"]$/g, ''));
  }
  return [...(npm || []), ...pnpm];
}
function matchPattern(pattern, relative) {
  const a = pattern.replace(/^\.\//, '').replace(/\/$/, '').split('/');
  const b = relative.split('/');
  function matches(i, j) {
    if (i === a.length) return j === b.length;
    if (a[i] === '**') return matches(i + 1, j) || (j < b.length && matches(i, j + 1));
    if (j === b.length) return false;
    const escaped = a[i].split('*').map((piece) => piece.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('.*');
    return new RegExp(`^${escaped}$`).test(b[j]) && matches(i + 1, j + 1);
  }
  return matches(0, 0);
}
const patterns = workspacePatterns();
const packages = new Map();
const named = new Map();
for (const [dir, name] of candidatePackages) {
  const relative = path.relative(root, dir).split(path.sep).join('/');
  const included = patterns.some((pattern) => !pattern.startsWith('!') && matchPattern(pattern, relative));
  const excluded = patterns.some((pattern) => pattern.startsWith('!') && matchPattern(pattern.slice(1), relative));
  if (included && !excluded) named.set(name, [...(named.get(name) || []), dir]);
}
for (const [name, dirs] of named) if (dirs.length === 1) packages.set(name, dirs[0]);
const virtualOutputs = new Map();
for (const dir of packages.values()) {
  const configFile = path.join(dir, 'tsconfig.json');
  if (!fs.existsSync(configFile)) continue;
  const own = ts.readConfigFile(configFile, ts.sys.readFile);
  if (own.error) continue;
  const parsedOwn = ts.parseJsonConfigFileContent(own.config, ts.sys, dir);
  const outDir = parsedOwn.options.outDir;
  const rootDir = parsedOwn.options.rootDir || (parsedOwn.options.composite ? dir : undefined);
  if (!outDir || !rootDir) continue;
  for (const source of indexed) {
    if (!source.startsWith(`${rootDir}${path.sep}`)) continue;
    const relative = path.relative(rootDir, source);
    const extension = source.endsWith('.mts') ? '.mjs' : source.endsWith('.cts') ? '.cjs' : '.js';
    const output = path.join(outDir, relative.replace(/\.(tsx?|jsx?|mts|cts)$/, extension));
    virtualOutputs.set(output, source);
  }
}
function realPath(candidate) {
  const marker = `${path.sep}node_modules${path.sep}`;
  const offset = candidate.indexOf(marker);
  if (offset < 0) return virtualOutputs.get(candidate) || candidate;
  const tail = candidate.slice(offset + marker.length);
  const parts = tail.split(path.sep);
  const name = parts[0].startsWith('@') ? parts.slice(0, 2).join('/') : parts[0];
  const dir = packages.get(name);
  if (!dir) return candidate;
  const real = path.join(dir, ...parts.slice(name.startsWith('@') ? 2 : 1));
  return virtualOutputs.get(real) || real;
}
function virtualDirectory(candidate) {
  if (candidate === path.join(root, 'node_modules')) return packages.size > 0;
  const prefix = `${path.join(root, 'node_modules')}${path.sep}`;
  if (!candidate.startsWith(prefix)) return false;
  const tail = candidate.slice(prefix.length);
  return [...packages.keys()].some((name) => name === tail || name.startsWith(`${tail}/`));
}
const host = {
  ...ts.sys,
  fileExists: (file) => ts.sys.fileExists(realPath(file)),
  directoryExists: (dir) => virtualDirectory(dir)
    || [...virtualOutputs.keys()].some((output) => output.startsWith(`${realPath(dir)}${path.sep}`))
    || ts.sys.directoryExists(realPath(dir)),
  readFile: (file) => ts.sys.readFile(realPath(file)),
  realpath: (file) => realPath(file),
  getDirectories: (dir) => ts.sys.getDirectories(realPath(dir)),
};
const rows = [];
for (const file of [...indexed].sort()) {
  if (filterPrefix && !path.relative(root, file).split(path.sep).join('/').startsWith(filterPrefix)) continue;
  const options = configFor(file);
  if (!options) continue;
  const source = ts.createSourceFile(file, fs.readFileSync(file, 'utf8'), ts.ScriptTarget.Latest, true);
  source.impliedNodeFormat = ts.getImpliedNodeFormatForFile(file, undefined, ts.sys, options);
  function record(node, literal, forcedMode) {
    if (!literal || !ts.isStringLiteralLike(literal)) return;
    const mode = forcedMode ?? ts.getModeForUsageLocation(source, node, options);
    const found = ts.resolveModuleName(literal.text, file, options, host, undefined, undefined, mode).resolvedModule;
    const realTarget = found && realPath(found.resolvedFileName);
    const target = realTarget && indexed.has(realTarget)
      ? path.relative(root, realTarget).split(path.sep).join('/')
      : '';
    const line = source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
    rows.push([caseName, path.relative(root, file).split(path.sep).join('/'), line, literal.text, target]);
  }
  function visit(node) {
    if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) {
      record(node, node.moduleSpecifier);
    } else if (ts.isImportEqualsDeclaration(node) && ts.isExternalModuleReference(node.moduleReference)) {
      record(node, node.moduleReference.expression, ts.ModuleKind.CommonJS);
    } else if (ts.isCallExpression(node)) {
      if (node.expression.kind === ts.SyntaxKind.ImportKeyword) {
        record(node, node.arguments[0], ts.ModuleKind.ESNext);
      } else if (ts.isIdentifier(node.expression) && node.expression.text === 'require') {
        record(node, node.arguments[0], ts.ModuleKind.CommonJS);
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
}
const body = rows.sort((a, b) => a[1].localeCompare(b[1]) || a[2] - b[2])
  .map((row) => `${row.join('\t')}\n`).join('');
if (outputPath) fs.writeFileSync(outputPath, body);
else process.stdout.write(body);
