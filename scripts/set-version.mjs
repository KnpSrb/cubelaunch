// Меняет версию сразу в package.json, tauri.conf.json и Cargo.toml.
// Использование: npm run version:set 0.2.0
import fs from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+(-[\w.]+)?$/.test(version ?? "")) {
  console.error("Укажите версию в виде 1.2.3");
  process.exit(1);
}
const file = (p) => new URL(`../${p}`, import.meta.url);

for (const p of ["package.json", "src-tauri/tauri.conf.json"]) {
  const json = JSON.parse(fs.readFileSync(file(p), "utf8"));
  json.version = version;
  fs.writeFileSync(file(p), JSON.stringify(json, null, 2) + "\n");
}

const lock = JSON.parse(fs.readFileSync(file("package-lock.json"), "utf8"));
lock.version = version;
if (lock.packages?.[""]) lock.packages[""].version = version;
fs.writeFileSync(file("package-lock.json"), JSON.stringify(lock, null, 2) + "\n");

const cargo = fs.readFileSync(file("src-tauri/Cargo.toml"), "utf8");
// Первая строка version = "…" — это секция [package].
fs.writeFileSync(file("src-tauri/Cargo.toml"), cargo.replace(/^version = ".*"$/m, `version = "${version}"`));

console.log(`версия ${version}; Cargo.lock обновится при следующей сборке`);
