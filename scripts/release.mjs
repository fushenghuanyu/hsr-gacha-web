#!/usr/bin/env node
/**
 * 一键构建分发包：编译前端与启动器 → 输出到 out/ → 打包 zip 到 out/zip/
 *
 * 用法（在项目根目录）:
 *   node scripts/release.mjs
 *   node scripts/release.mjs --no-build
 *   node scripts/release.mjs --npm-install
 *   node scripts/release.mjs --skip-zip
 */

import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  cpSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const PROJECT_ROOT = resolve(SCRIPT_DIR, "..");
const FRONTEND_DIR = join(PROJECT_ROOT, "frontend");
const BACKEND_DIR = join(PROJECT_ROOT, "backend-rust");
const OUT_DIR = join(PROJECT_ROOT, "out");
const OUT_ZIP_DIR = join(OUT_DIR, "zip");

const LAUNCHER_STEM = "hsr-gacha-launcher";
const ZIP_PREFIX = "hsr-gacha-distribution";
const IS_WIN = process.platform === "win32";
const LAUNCHER_FILENAME = LAUNCHER_STEM + (IS_WIN ? ".exe" : "");

const ARTIFACT_NAMES = [LAUNCHER_FILENAME, "dist", "resources", "VERSION"];

function parseArgs(argv) {
  return {
    noBuild: argv.includes("--no-build"),
    npmInstall: argv.includes("--npm-install"),
    skipZip: argv.includes("--skip-zip"),
  };
}

function readVersion() {
  const candidates = [
    join(PROJECT_ROOT, "..", "VERSION"),
    join(FRONTEND_DIR, "package.json"),
  ];
  for (const path of candidates) {
    if (!existsSync(path)) continue;
    const raw = readFileSync(path, "utf8");
    if (path.endsWith("package.json")) {
      const version = JSON.parse(raw).version;
      if (version) return String(version).trim();
    } else {
      const version = raw.trim();
      if (version) return version;
    }
  }
  console.error("[error] 无法读取版本号（仓库根 VERSION 或 frontend/package.json）");
  process.exit(1);
}

function requireTool(name) {
  const cmd = IS_WIN ? "where" : "which";
  const result = spawnSync(cmd, [name], { encoding: "utf8" });
  if (result.status !== 0) {
    console.error(`[error] 未找到 ${name}，请先安装对应工具链。`);
    process.exit(1);
  }
}

function run(cmd, args, cwd, label) {
  console.log(`>>> [${label}] ${cmd} ${args.join(" ")}`);
  const result = spawnSync(cmd, args, { cwd, stdio: "inherit", shell: IS_WIN });
  if (result.status !== 0) {
    console.error(`[error] 命令失败 (exit ${result.status}): ${label}`);
    process.exit(result.status ?? 1);
  }
}

function build({ npmInstall }) {
  requireTool("npm");
  requireTool("cargo");

  if (npmInstall || !existsSync(join(FRONTEND_DIR, "node_modules"))) {
    run("npm", ["install"], FRONTEND_DIR, "npm install");
  }
  run("npm", ["run", "build"], FRONTEND_DIR, "npm run build");
  run(
    "cargo",
    ["build", "--release", "--bin", LAUNCHER_STEM],
    BACKEND_DIR,
    "cargo build launcher",
  );
}

function verifyArtifacts() {
  const distIndex = join(PROJECT_ROOT, "dist", "index.html");
  const exePath = join(BACKEND_DIR, "target", "release", LAUNCHER_FILENAME);

  if (!existsSync(distIndex)) {
    console.error(
      `[error] 缺少前端构建产物: ${distIndex}\n  请在 frontend 目录执行 npm run build。`,
    );
    process.exit(1);
  }
  if (!existsSync(exePath)) {
    console.error(
      `[error] 缺少启动器: ${exePath}\n  cargo build --release --bin ${LAUNCHER_STEM}`,
    );
    process.exit(1);
  }
  return exePath;
}

function isFileLockedError(err) {
  return err && (err.code === "EPERM" || err.code === "EBUSY" || err.code === "EACCES");
}

/** Windows 上正在运行的 exe 无法直接删除，可先重命名让位。 */
function removeWindowsExe(path) {
  const stalePath = `${path}.old`;
  if (existsSync(stalePath)) {
    try {
      rmSync(stalePath, { force: true });
    } catch {
      // 旧的 .old 仍被占用时忽略，继续尝试覆盖 rename 目标
    }
  }
  renameSync(path, stalePath);
  console.log(`  提示: 旧启动器可能仍在运行，已重命名为 ${basename(stalePath)}`);
  try {
    rmSync(stalePath, { force: true });
  } catch {
    console.log(`  提示: 可稍后手动删除 ${basename(stalePath)}`);
  }
}

function removePath(path) {
  if (!existsSync(path)) return;
  try {
    rmSync(path, { recursive: true, force: true });
    return;
  } catch (err) {
    const isWinExe =
      IS_WIN && path.toLowerCase().endsWith(".exe") && statSync(path).isFile();
    if (isWinExe && isFileLockedError(err)) {
      removeWindowsExe(path);
      return;
    }
    if (isFileLockedError(err)) {
      console.error(
        `[error] 无法删除 ${path}（文件被占用）。\n` +
          `  若正在运行 ${basename(path)}，请先关闭后再执行 release。`,
      );
      process.exit(1);
    }
    throw err;
  }
}

function copyLauncherExe(src, dest) {
  try {
    copyFileSync(src, dest);
    return;
  } catch (err) {
    if (!IS_WIN || !isFileLockedError(err)) throw err;
  }

  if (existsSync(dest)) {
    removeWindowsExe(dest);
  }
  try {
    copyFileSync(src, dest);
  } catch (err) {
    console.error(
      `[error] 无法写入 ${dest}。\n` +
        `  请先关闭正在运行的 ${LAUNCHER_FILENAME}（任务管理器结束进程）后重试。`,
    );
    process.exit(1);
  }
}

function clearDistributionArtifacts(dest) {
  if (!existsSync(dest)) return;
  for (const name of ARTIFACT_NAMES) {
    const path = join(dest, name);
    if (!existsSync(path)) continue;
    console.log(`  清理: ${path}`);
    removePath(path);
  }
}

function stageDistribution(dest) {
  const exePath = verifyArtifacts();
  mkdirSync(dest, { recursive: true });

  const userData = join(dest, "userData");
  if (existsSync(userData)) {
    console.log(`  保留: ${userData}`);
  }
  clearDistributionArtifacts(dest);

  copyLauncherExe(exePath, join(dest, LAUNCHER_FILENAME));
  console.log(`  已写入: ${LAUNCHER_FILENAME}`);

  cpSync(join(PROJECT_ROOT, "dist"), join(dest, "dist"), { recursive: true });
  console.log("  已写入: dist/");

  const resourcesSrc = join(PROJECT_ROOT, "resources");
  if (existsSync(resourcesSrc)) {
    cpSync(resourcesSrc, join(dest, "resources"), { recursive: true });
    console.log("  已写入: resources/");
  } else {
    mkdirSync(join(dest, "resources", "icon"), { recursive: true });
    console.log("  提示: 无 resources/，已创建空目录");
  }

  const version = readVersion();
  writeFileSync(join(dest, "VERSION"), `${version}\n`, "utf8");
  console.log(`  已写入: VERSION (v${version})`);
}

function createZipFromDir(sourceDir, zipPath) {
  mkdirSync(dirname(zipPath), { recursive: true });
  if (existsSync(zipPath)) removePath(zipPath);

  if (IS_WIN) {
    const ps = [
      "-NoProfile",
      "-Command",
      `Compress-Archive -Path '${sourceDir.replace(/'/g, "''")}\\*' -DestinationPath '${zipPath.replace(/'/g, "''")}' -Force`,
    ];
    run("powershell", ps, PROJECT_ROOT, "zip");
    return;
  }

  const zipCmd = spawnSync("which", ["zip"], { encoding: "utf8" });
  if (zipCmd.status !== 0) {
    console.error("[error] 未找到 zip 命令，请安装 zip 或使用 Windows 环境。");
    process.exit(1);
  }
  run("zip", ["-r", zipPath, "."], sourceDir, "zip");
}

/** 仅打包分发文件，不包含 out/zip/ 等目录。 */
function createZip(dest, zipPath) {
  const tmp = mkdtempSync(join(tmpdir(), "hsr-gacha-pack-"));
  try {
    for (const name of ARTIFACT_NAMES) {
      const src = join(dest, name);
      if (!existsSync(src)) continue;
      const dst = join(tmp, name);
      cpSync(src, dst, { recursive: true });
    }
    createZipFromDir(tmp, zipPath);
  } finally {
    removePath(tmp);
  }
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const version = readVersion();

  console.log(`项目根目录: ${PROJECT_ROOT}`);
  console.log(`产品版本: v${version}`);

  if (!args.noBuild) {
    console.log("\n=== 编译前端与启动器 ===");
    build({ npmInstall: args.npmInstall });
  }

  console.log(`\n=== 输出到 ${OUT_DIR} ===`);
  mkdirSync(OUT_DIR, { recursive: true });
  stageDistribution(OUT_DIR);
  console.log(`\n完成。在 out/ 目录运行: ${join(OUT_DIR, LAUNCHER_FILENAME)}`);

  if (!args.skipZip) {
    console.log(`\n=== 打包 zip 到 ${OUT_ZIP_DIR} ===`);
    const zipName = `${ZIP_PREFIX}-v${version}.zip`;
    const zipPath = join(OUT_ZIP_DIR, zipName);
    createZip(OUT_DIR, zipPath);
    console.log(`  已生成: ${zipPath}`);
  }

  console.log("\n发布完成");
}

main();
