#!/usr/bin/env node
/**
 * Builds the claude-code-bridge sidecar binary using @yao-pkg/pkg.
 * Output goes to src-tauri/binaries/ with the correct target triple suffix.
 */
import { execSync } from "child_process";
import { existsSync, mkdirSync, renameSync } from "fs";
import { dirname, join, resolve } from "path";
import { fileURLToPath } from "url";
import os from "os";

const __dirname = dirname(fileURLToPath(import.meta.url));
const bridgeRoot = resolve(__dirname, "..");
const binariesDir = resolve(bridgeRoot, "../../binaries");
const distEntry = resolve(bridgeRoot, "dist/index.js");

function getTargetTriple() {
  const platform = os.platform();
  const arch = os.arch();

  const platformMap = { darwin: "apple-darwin", linux: "unknown-linux-gnu", win32: "pc-windows-msvc" };
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
    execSync("npx tsc", { cwd: bridgeRoot, stdio: "inherit" });
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

  const pkgTarget = os.arch() === "arm64" ? "node20-macos-arm64" : `node20-${os.platform()}-${os.arch()}`;

  execSync(
    `npx @yao-pkg/pkg dist/index.js --target ${pkgTarget} --output pkg-output${ext}`,
    { cwd: bridgeRoot, stdio: "inherit" },
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
