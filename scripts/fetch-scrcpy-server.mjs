// Baixa o scrcpy-server que o Spiegel embute, fixado na v4.1 (ADR 0001).
// Uso: pnpm fetch:server [--force]
// O arquivo fica em src-tauri/resources/scrcpy-server, fora do git, e o
// download é conferido pelo SHA-256 publicado no release oficial.
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const VERSION = "4.1";
const SHA256 = "deacb991ed2509715160ffdc7907e47b4160eb30d1566217e9047fd5b8850cae";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const dest = join(root, "src-tauri", "resources", "scrcpy-server");

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

const force = process.argv.includes("--force");
if (!force && existsSync(dest) && sha256(readFileSync(dest)) === SHA256) {
  console.log(`scrcpy-server ${VERSION} já está em ${dest} (use --force para baixar de novo)`);
  process.exit(0);
}

const url = `https://github.com/Genymobile/scrcpy/releases/download/v${VERSION}/scrcpy-server-v${VERSION}`;
console.log(`baixando ${url}`);
const response = await fetch(url);
if (!response.ok) throw new Error(`download falhou: ${response.status} ${response.statusText}`);

const bytes = Buffer.from(await response.arrayBuffer());
const actual = sha256(bytes);
if (actual !== SHA256) throw new Error(`SHA-256 inesperado: ${actual} (esperado ${SHA256})`);

mkdirSync(dirname(dest), { recursive: true });
writeFileSync(dest, bytes);
console.log(`scrcpy-server ${VERSION} em ${dest}`);
