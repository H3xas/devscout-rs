#!/usr/bin/env node
// Resolve the recorded module sites with TypeScript's own API.
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
const modeNames = new Map([
  [ts.ModuleResolutionKind.Classic, 'classic'],
  [ts.ModuleResolutionKind.Node10, 'node10'],
  [ts.ModuleResolutionKind.Node16, 'node16'],
  [ts.ModuleResolutionKind.NodeNext, 'nodenext'],
  [ts.ModuleResolutionKind.Bundler, 'bundler'],
]);
for (const file of [...indexed].sort()) {
  if (filterPrefix && !path.relative(root, file).split(path.sep).join('/').startsWith(filterPrefix)) continue;
  const options = configFor(file);
  if (!options) continue;
  const source = ts.createSourceFile(file, fs.readFileSync(file, 'utf8'), ts.ScriptTarget.Latest, true);
  source.impliedNodeFormat = ts.getImpliedNodeFormatForFile(file, undefined, ts.sys, options);
  function record(node, literal, form, forcedMode) {
    const valid = literal && (ts.isStringLiteral(literal)
      || (form === 'dynamic' && ts.isNoSubstitutionTemplateLiteral(literal)));
    if (!valid && form !== 'dynamic') return;
    const mode = forcedMode ?? ts.getModeForUsageLocation(source, node, options);
    const found = valid
      ? ts.resolveModuleName(literal.text, file, options, host, undefined, undefined, mode).resolvedModule
      : undefined;
    const rawTarget = found && (found.originalPath || found.resolvedFileName);
    const marker = `${path.sep}node_modules${path.sep}`;
    const offset = rawTarget ? rawTarget.indexOf(marker) : -1;
    let link = '';
    let linkedTarget = rawTarget;
    if (offset >= 0) {
      const parts = rawTarget.slice(offset + marker.length).split(path.sep);
      const name = parts[0].startsWith('@') ? parts.slice(0, 2).join('/') : parts[0];
      const member = packages.get(name);
      if (member) {
        link = path.relative(root, member).split(path.sep).join('/');
        linkedTarget = path.join(member, ...parts.slice(name.startsWith('@') ? 2 : 1));
      }
    }
    const mapped = linkedTarget && virtualOutputs.has(linkedTarget) ? '1' : '';
    const realTarget = found && realPath(found.resolvedFileName);
    const target = realTarget && indexed.has(realTarget)
      ? path.relative(root, realTarget).split(path.sep).join('/')
      : '';
    const line = source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
    rows.push([caseName, path.relative(root, file).split(path.sep).join('/'), line,
      valid ? literal.text : '(non-literal)', target, valid ? form : 'dynamic-non-literal',
      modeNames.get(ts.getEmitModuleResolutionKind(options)) || '', link, mapped]);
  }
  for (const statement of source.statements) {
    if (ts.isImportDeclaration(statement)) record(statement, statement.moduleSpecifier, 'import');
    else if (ts.isExportDeclaration(statement)) record(statement, statement.moduleSpecifier, 'export');
    else if (ts.isImportEqualsDeclaration(statement) && ts.isExternalModuleReference(statement.moduleReference)
      && statement.name && !(ts.getModifiers(statement) || []).some((modifier) => modifier.kind === ts.SyntaxKind.ExportKeyword)) {
      record(statement, statement.moduleReference.expression, 'import-equals', ts.ModuleKind.CommonJS);
    } else if (ts.isVariableStatement(statement)
      && !(ts.getModifiers(statement) || []).some((modifier) => modifier.kind === ts.SyntaxKind.ExportKeyword)) {
      for (const declaration of statement.declarationList.declarations) {
        const call = declaration.initializer;
        if (call && ts.isCallExpression(call) && ts.isIdentifier(call.expression)
          && call.expression.text === 'require') {
          record(declaration, call.arguments[0], 'require', ts.ModuleKind.CommonJS);
        }
      }
    }
  }
  function visit(node) {
    if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword) {
      record(node, node.arguments[0], 'dynamic', ts.ModuleKind.ESNext);
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
}
const body = rows.sort((a, b) => a[1].localeCompare(b[1]) || a[2] - b[2]
  || a[3].localeCompare(b[3]))
  .map((row) => `${row.join('\t')}\n`).join('');
if (outputPath) fs.writeFileSync(outputPath, body);
else process.stdout.write(body);
