'use strict';

const path = require('path');

const PLATFORMS = {
  'linux-x64': '@lazypaw/linux-x64',
  'linux-arm64': '@lazypaw/linux-arm64',
  'darwin-x64': '@lazypaw/darwin-x64',
  'darwin-arm64': '@lazypaw/darwin-arm64',
  'win32-x64': '@lazypaw/win32-x64',
  'win32-arm64': '@lazypaw/win32-arm64',
};

function getBinaryPath() {
  const platformKey = `${process.platform}-${process.arch}`;
  const pkg = PLATFORMS[platformKey];

  if (!pkg) {
    throw new Error(
      `Unsupported platform: ${platformKey}. Supported: ${Object.keys(PLATFORMS).join(', ')}`
    );
  }

  const pkgPath = require.resolve(`${pkg}/package.json`);
  const binName = process.platform === 'win32' ? 'lazypaw.exe' : 'lazypaw';
  return path.join(path.dirname(pkgPath), 'bin', binName);
}

module.exports = { getBinaryPath, PLATFORMS };
