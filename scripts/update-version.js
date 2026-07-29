#!/usr/bin/env node
// Bumps the version everywhere it is recorded. Called by semantic-release's
// prepareCmd with the next version as the first argument.
//
// Unlike Intentio Mind Map, the Rust side here is a workspace: the version lives
// once in the root Cargo.toml under [workspace.package], and every crate inherits
// it. Cargo.lock records each crate's version too, so it is rewritten in step —
// otherwise the first build after a release would dirty the tree.
const { execSync } = require('child_process');
const fs = require('fs');

const version = process.argv[2];
if (!version) {
  console.error('Usage: update-version.js <version>');
  process.exit(1);
}

// npm handles package.json + package-lock.json
execSync(`npm version ${version} --no-git-tag-version`, { stdio: 'inherit' });

// tauri.conf.json
const tauriConf = JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json', 'utf8'));
tauriConf.version = version;
fs.writeFileSync('src-tauri/tauri.conf.json', JSON.stringify(tauriConf, null, 2) + '\n');

// Root Cargo.toml — the [workspace.package] version every crate inherits.
let cargo = fs.readFileSync('Cargo.toml', 'utf8');
const before = cargo;
cargo = cargo.replace(
  /(\[workspace\.package\][\s\S]*?^version = ")[^"]*(")/m,
  `$1${version}$2`
);
if (cargo === before) {
  console.error('Could not find [workspace.package] version in Cargo.toml');
  process.exit(1);
}
fs.writeFileSync('Cargo.toml', cargo);

// Cargo.lock — the workspace members carry the same version.
const MEMBERS = ['int-vault', 'int-knowledge-mcp', 'int-knowledge'];
let lock = fs.readFileSync('Cargo.lock', 'utf8');
for (const member of MEMBERS) {
  // Match the `version` line inside this member's [[package]] block only.
  const pattern = new RegExp(
    `(\\[\\[package\\]\\]\\nname = "${member}"\\nversion = ")[^"]*(")`,
    'g'
  );
  if (!pattern.test(lock)) {
    console.warn(`Warning: ${member} not found in Cargo.lock`);
    continue;
  }
  lock = lock.replace(
    new RegExp(`(\\[\\[package\\]\\]\\nname = "${member}"\\nversion = ")[^"]*(")`, 'g'),
    `$1${version}$2`
  );
}
fs.writeFileSync('Cargo.lock', lock);

console.log(`Bumped all version files to ${version}`);
