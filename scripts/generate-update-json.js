#!/usr/bin/env node
// Runs after all platform builds complete. Downloads .sig files from the GitHub
// release, constructs the Tauri updater JSON, and writes it to latest.json.
const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const tag = process.argv[2]; // e.g. "v1.2.3"
const repo = process.env.GITHUB_REPOSITORY; // e.g. "owner/repo"

if (!tag || !repo) {
  console.error('Usage: GITHUB_REPOSITORY=owner/repo generate-update-json.js <tag>');
  process.exit(1);
}

const version = tag.replace(/^v/, '');

// Map .sig filename suffixes to Tauri platform keys.
// Universal macOS DMG covers both darwin arches.
const PLATFORM_PATTERNS = [
  { platforms: ['darwin-aarch64', 'darwin-x86_64'], pattern: /universal\.app\.tar\.gz\.sig$/ },
  { platforms: ['darwin-aarch64', 'darwin-x86_64'], pattern: /universal\.dmg\.sig$/ },
  { platforms: ['linux-x86_64'], pattern: /amd64\.AppImage\.sig$/ },
  { platforms: ['windows-x86_64'], pattern: /(x64_en-US\.msi|x64-setup\.exe)\.sig$/ },
];

// Get the list of release assets
const assetsJson = execSync(`gh release view ${tag} --json assets`, {
  env: process.env,
}).toString();
const assets = JSON.parse(assetsJson).assets;

const releaseInfo = JSON.parse(
  execSync(`gh release view ${tag} --json createdAt,body`).toString()
);

// Download .sig files into a temp dir
const tmpDir = path.join(process.cwd(), 'tmp-sigs');
fs.mkdirSync(tmpDir, { recursive: true });

const sigAssets = assets.filter((a) => a.name.endsWith('.sig'));
if (sigAssets.length === 0) {
  console.error('No .sig files found in release assets — did the builds complete?');
  process.exit(1);
}

execSync(
  `gh release download ${tag} --pattern "*.sig" --dir ${tmpDir} --clobber`,
  { env: process.env, stdio: 'inherit' }
);

const platforms = {};

for (const sigAsset of sigAssets) {
  const mapping = PLATFORM_PATTERNS.find((p) => p.pattern.test(sigAsset.name));
  if (!mapping) {
    console.warn(`No platform mapping for ${sigAsset.name}, skipping`);
    continue;
  }

  const installerName = sigAsset.name.replace(/\.sig$/, '');
  const installer = assets.find((a) => a.name === installerName);
  if (!installer) {
    console.warn(`Installer not found for ${sigAsset.name}, skipping`);
    continue;
  }

  const sig = fs.readFileSync(path.join(tmpDir, sigAsset.name), 'utf8').trim();
  const url = `https://github.com/${repo}/releases/download/${tag}/${installerName}`;

  for (const platform of mapping.platforms) {
    platforms[platform] = { signature: sig, url };
  }
}

if (Object.keys(platforms).length === 0) {
  console.error('No platform entries generated — check .sig file naming');
  process.exit(1);
}

const json = {
  version,
  notes: releaseInfo.body ?? '',
  pub_date: releaseInfo.createdAt,
  platforms,
};

fs.writeFileSync('latest.json', JSON.stringify(json, null, 2));
fs.rmSync(tmpDir, { recursive: true });

console.log('Generated latest.json:');
console.log(JSON.stringify(json, null, 2));
