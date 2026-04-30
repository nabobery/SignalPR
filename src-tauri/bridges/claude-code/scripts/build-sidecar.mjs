#!/usr/bin/env node
/**
 * Builds the claude-code-bridge sidecar binary using @yao-pkg/pkg.
 * Output goes to src-tauri/binaries/ with the correct target triple suffix.
 */
import { execFileSync } from "child_process";
import { existsSync, mkdirSync, renameSync } from "fs";
import { dirname, join, resolve } from "path";
import { fileURLToPath } from "url";
import os from "os";
import { createRequire } from "module";

const __dirname = dirname(fileURLToPath(import.meta.url));
const bridgeRoot = resolve(__dirname, "..");
const binariesDir = resolve(bridgeRoot, "../../binaries");
const distEntry = resolve(bridgeRoot, "dist/index.js");
const require = createRequire(import.meta.url);

function cleanChildEnv() {
  return Object.fromEntries(
    Object.entries(process.env).filter(([key]) => !key.startsWith("npm_config_")),
  );
}

function runNodeBin(packageName, args, options = {}) {
  const packageJsonPath = require.resolve(`${packageName}/package.json`);
  const packageJson = require(packageJsonPath);
  const binEntry =
    typeof packageJson.bin === "string"
      ? packageJson.bin
      : (packageJson.bin?.[packageJson.name.split("/").at(-1)] ??
        Object.values(packageJson.bin ?? {})[0]);

  if (!binEntry) {
    throw new Error(`Unable to resolve executable for ${packageName}`);
  }

  const binPath = resolve(dirname(packageJsonPath), binEntry);
  execFileSync(process.execPath, [binPath, ...args], {
    stdio: "inherit",
    env: cleanChildEnv(),
    ...options,
  });
}

function getTargetTriple() {
  const platform = os.platform();
  const arch = os.arch();

  const platformMap = {
    darwin: "apple-darwin",
    linux: "unknown-linux-gnu",
    win32: "pc-windows-msvc",
  };
  const archMap = { x64: "x86_64", arm64: "aarch64" };

  const p = platformMap[platform];
  const a = archMap[arch];
  if (!p || !a) throw new Error(`Unsupported platform: ${platform}-${arch}`);
  return `${a}-${p}`;
}

function main() {
  process.env.PKG_CACHE_PATH ??= join(os.tmpdir(), "signalpr-pkg-cache");

  if (!existsSync(distEntry)) {
    console.log("Compiling TypeScript...");
    runNodeBin("typescript", [], { cwd: bridgeRoot });
  }

  if (!existsSync(binariesDir)) {
    mkdirSync(binariesDir, { recursive: true });
  }

  const triple = getTargetTriple();
  const ext = os.platform() === "win32" ? ".exe" : "";
  const outputName = `claude-code-bridge-${triple}${ext}`;
  const outputPath = join(binariesDir, outputName);

  console.log(`Building sidecar for ${triple}...`);
  console.log(`Output: ${outputPath}`);

  const pkgTarget =
    os.arch() === "arm64" ? "node20-macos-arm64" : `node20-${os.platform()}-${os.arch()}`;

  runNodeBin(
    "@yao-pkg/pkg",
    ["dist/index.js", "--target", pkgTarget, "--output", `pkg-output${ext}`],
    {
      cwd: bridgeRoot,
    },
  );

  const pkgOutput = join(bridgeRoot, `pkg-output${ext}`);
  if (existsSync(pkgOutput)) {
    renameSync(pkgOutput, outputPath);
    console.log(`Sidecar built: ${outputPath}`);
  } else {
    throw new Error("pkg did not produce expected output file");
  }
}

main();
