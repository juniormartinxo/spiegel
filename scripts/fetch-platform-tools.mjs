// Baixa o adb (platform-tools do Google) que o Spiegel embute.
// Uso: pnpm fetch:adb [--force]
// Os binários ficam em src-tauri/resources/platform-tools, fora do git.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, copyFileSync, rmSync, writeFileSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const dest = join(root, "src-tauri", "resources", "platform-tools");

const platforms = {
  win32: { zip: "windows", files: ["adb.exe", "AdbWinApi.dll", "AdbWinUsbApi.dll"] },
  linux: { zip: "linux", files: ["adb"] },
  darwin: { zip: "darwin", files: ["adb"] },
};
const platform = platforms[process.platform];
if (!platform) throw new Error(`plataforma não suportada: ${process.platform}`);

const force = process.argv.includes("--force");
if (!force && platform.files.every((file) => existsSync(join(dest, file)))) {
  console.log(`adb já está em ${dest} (use --force para baixar de novo)`);
  process.exit(0);
}

const url = `https://dl.google.com/android/repository/platform-tools-latest-${platform.zip}.zip`;
console.log(`baixando ${url}`);
const response = await fetch(url);
if (!response.ok) throw new Error(`download falhou: ${response.status} ${response.statusText}`);

const work = mkdtempSync(join(tmpdir(), "spiegel-platform-tools-"));
try {
  const zipPath = join(work, "platform-tools.zip");
  writeFileSync(zipPath, Buffer.from(await response.arrayBuffer()));
  // O tar do Windows 10+ (bsdtar) e o do macOS abrem zip; no Linux usa-se o unzip.
  // No Windows, o caminho é explícito: o `tar` do Git Bash (GNU) não abre zip.
  if (process.platform === "linux") execFileSync("unzip", ["-q", zipPath, "-d", work]);
  else if (process.platform === "win32") execFileSync(join(process.env.SystemRoot ?? "C:/Windows", "System32", "tar.exe"), ["-xf", zipPath, "-C", work]);
  else execFileSync("tar", ["-xf", zipPath, "-C", work]);

  mkdirSync(dest, { recursive: true });
  for (const file of [...platform.files, "NOTICE.txt", "source.properties"]) {
    copyFileSync(join(work, "platform-tools", file), join(dest, file));
  }
  const revision = readFileSync(join(dest, "source.properties"), "utf8").match(/Pkg\.Revision=(.+)/)?.[1];
  console.log(`platform-tools ${revision ?? "?"} em ${dest}`);
} finally {
  rmSync(work, { recursive: true, force: true });
}
