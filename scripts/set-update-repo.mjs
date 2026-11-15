// Подставляет репозиторий GitHub в адрес обновлений и домашнюю страницу.
// Использование: node scripts/set-update-repo.mjs owner/repo
import fs from "node:fs";

const repo = process.argv[2];
if (!/^[\w.-]+\/[\w.-]+$/.test(repo ?? "")) {
  console.error("Укажите репозиторий в виде owner/repo");
  process.exit(1);
}
const path = new URL("../src-tauri/tauri.conf.json", import.meta.url);
const conf = JSON.parse(fs.readFileSync(path, "utf8"));
conf.plugins.updater.endpoints = [`https://github.com/${repo}/releases/latest/download/latest.json`];
conf.bundle.homepage = `https://github.com/${repo}`;
fs.writeFileSync(path, JSON.stringify(conf, null, 2) + "\n");
console.log(`адрес обновлений: ${conf.plugins.updater.endpoints[0]}`);
