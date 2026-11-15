# Cubelaunch

Кроссплатформенный лаунчер Minecraft: Java Edition на Tauri 2 (Rust) + React + TypeScript.
Построен вокруг **инстансов** — изолированных установок игры со своими модами, мирами и настройками.

> Статус: все 9 этапов — каркас, настройки, i18n, тема; манифест Mojang, инстансы и группы,
> загрузка клиента, библиотек и assets с проверкой SHA1; поиск и автоскачивание Java (Temurin),
> запуск игры, лог в реальном времени, перехват крашей; вход через Microsoft,
> мультиаккаунт, офлайн-аккаунты, 3D-просмотр скина; загрузчики модов Fabric, Quilt, Forge, NeoForge;
> моды, ресурспаки, шейдеры и миры, поиск и установка с Modrinth и CurseForge, обновления;
> модпаки: установка, импорт и экспорт .mrpack / zip CurseForge, обновление;
> центр загрузок с очередью, группы с перетаскиванием, клонирование, экспорт и импорт инстансов (в т. ч. из Prism Launcher);
> автообновление лаунчера, установщики для Windows, macOS и Linux.

## Установка

Скачайте установщик со страницы релизов:

| ОС | Файл | Примечание |
|---|---|---|
| Windows 10/11 | `Cubelaunch_<версия>_x64-setup.exe` | ставится в профиль пользователя, права администратора не нужны; WebView2 докачивается при необходимости |
| macOS 10.15+ | `Cubelaunch_<версия>_aarch64.dmg` (Apple Silicon) / `_x64.dmg` (Intel) | без подписи Apple при первом запуске: правый клик → «Открыть» |
| Linux | `.AppImage`, `.deb` или `.rpm` | AppImage: `chmod +x` и запуск; для keyring нужен Secret Service (GNOME Keyring, KWallet) |

## Обновления

При запуске лаунчер проверяет `latest.json` последнего релиза GitHub (отключается в
*Настройки → О программе*). Если есть новая версия — сверху появляется полоса с описанием релиза
и кнопкой «Установить и перезапустить». Кнопка неактивна, пока запущена игра или идут загрузки.
Обновление скачивается, проверяется подписью (публичный ключ вшит в `tauri.conf.json`) и ставится:

- Windows — установщик NSIS в тихом режиме, лаунчер перезапускается сам;
- macOS — заменяется `.app`; Linux — заменяется AppImage, а `.deb`/`.rpm` ставятся через `pkexec`.

В портативном режиме и в отладочной сборке обновление не ставится автоматически — показывается
ссылка на страницу релиза.

## Загрузчики модов

Загрузчик выбирается при создании инстанса и меняется на вкладке «Версия» (вместе с версией Minecraft).
Профиль загрузчика (`versions/<загрузчик>-<mc>-<версия>/…json`) накладывается на ванильный по `inheritsFrom`.

| Загрузчик | Откуда | Как ставится |
|---|---|---|
| Fabric | meta.fabricmc.net, maven.fabricmc.net | готовый профиль + библиотеки |
| Quilt | meta.quiltmc.org, maven.quiltmc.org | готовый профиль + библиотеки |
| Forge | maven.minecraftforge.net | официальный установщик: лаунчер читает `install_profile.json`, качает библиотеки и запускает процессоры установщика своей Java (патчинг клиента делается локально) |
| NeoForge | maven.neoforged.net | так же, как Forge |

Поддерживаются и старые установщики Forge (до 1.12.2, формат `versionInfo`). Готовые процессоры
пропускаются, если их результат уже на месте и SHA1 совпадает, поэтому повторная проверка быстрая.

Проверка реальной установки всех загрузчиков (нужна сеть; Java скачается при необходимости):

```bash
cd src-tauri && cargo test -- --ignored install_real_loaders --nocapture
```

## Вход через Microsoft: Client ID

Для входа нужен **свой** Client ID приложения Azure — чужие (официального лаунчера и др.)
использовать нельзя.

1. [portal.azure.com](https://portal.azure.com) → *App registrations* → *New registration*.
   Тип учётных записей — *Personal Microsoft accounts only*; Redirect URI не нужен.
2. *Authentication* → *Advanced settings* → **Allow public client flows: Yes** (нужно для device code).
3. Скопируйте *Application (client) ID*.
4. Подайте заявку Mojang на доступ к Minecraft API для этого ID (форма: <https://aka.ms/mce-reviewappid>).
   Пока заявка не одобрена, вход дойдёт до шага Minecraft и завершится ошибкой 403 —
   лаунчер покажет это понятным сообщением.
5. Укажите ID одним из способов:
   - вшить при сборке: `CUBELAUNCH_MSA_CLIENT_ID=<id> npm run tauri build`;
   - или ввести в *Настройки → Вход через Microsoft*.

Токены (refresh-токен Microsoft и токен Minecraft) хранятся только в системном хранилище
секретов: Windows Credential Manager, macOS Keychain, Secret Service (gnome-keyring/KWallet) на Linux.
В `accounts.json` — лишь ник, UUID и ссылка на скин.

Проверка реальной загрузки с серверов Mojang (без объектов assets):

```bash
cd src-tauri && cargo test -- --ignored install_real_version --nocapture
```

## Моды, ресурспаки, шейдеры, миры

- Вкладки инстанса показывают содержимое `mods/`, `resourcepacks/`, `shaderpacks/` с названием, версией и иконкой
  из самого файла (`fabric.mod.json`, `quilt.mod.json`, `mods.toml`/`neoforge.mods.toml`, `mcmod.info`, `pack.mcmeta`).
- Выключение — суффикс `.disabled`; удаление — в корзину ОС; файлы можно перетащить в окно или выбрать в диалоге.
- Моды под другой загрузчик помечаются («другой загрузчик»); Quilt принимает моды Fabric, NeoForge 1.20.1 — моды Forge.
- **Modrinth**: поиск только по совместимым с версией игры и загрузчиком проектам, выбор версии, установка
  с обязательными зависимостями. Связь файла с проектом определяется по SHA1 через API Modrinth, поэтому
  распознаются и обновляются и моды, добавленные вручную. Выключенный мод после обновления остаётся выключенным.
- **Миры**: список с данными из `level.dat` (название, режим, хардкор, версия, последний запуск), резервная копия
  в `instances/<id>/backups/` (zip), удаление в корзину.

## Модпаки

- **Установка**: «Добавить инстанс» → «Модпаки» (поиск на Modrinth или CurseForge, выбор версии) или «Импорт файла»
  (`.mrpack` / zip CurseForge; файл можно перетащить на список инстансов). Модпак ставится как новый инстанс.
- **Обновление** — на вкладке «Версия» инстанса: выбор версии из источника или «Обновить из файла».
  Лаунчер помнит файлы, положенные из пака (`instances/<id>/modpack.json`: путь → SHA1), поэтому при обновлении
  файлы пака заменяются, убранные из пака удаляются, а ваши моды и изменённые вами конфиги остаются.
  Выключенный мод пака остаётся выключенным.
- **Экспорт** в `.mrpack` или zip CurseForge: файлы, найденные на Modrinth (по SHA1) или CurseForge (по отпечатку
  murmur2), записываются ссылками, остальное — в `overrides/`. Логи, скриншоты и файлы игры не экспортируются.
- Пути из архивов проверяются (защита от zip-slip), все файлы скачиваются по https с проверкой SHA1.

## CurseForge

Для CurseForge нужен ключ API (<https://console.curseforge.com>). Без ключа CurseForge отключён, остальное работает.

- Ввести ключ: *Настройки → CurseForge*. Ключ проверяется запросом к API и хранится в системном хранилище секретов.
- Или вшить при сборке: `CUBELAUNCH_CURSEFORGE_KEY=<ключ> npm run tauri build`.

Некоторые авторы запрещают скачивание своих файлов сторонними лаунчерами. Такие файлы модпака перечисляются
на вкладке «Версия» со ссылками: скачайте их в папку «Загрузки» и нажмите «Проверить» — лаунчер найдёт их
по имени и SHA1 и положит на место.

## Требования

- Node.js 20+
- Rust stable (MSVC-тулчейн на Windows) и [зависимости Tauri](https://v2.tauri.app/start/prerequisites/)

## Запуск

```bash
npm install
npm run tauri dev      # приложение целиком
npm run dev            # только UI в браузере (мок-бэкенд, см. src/lib/mock.ts)
cd src-tauri && cargo test
```

## Сборка установщиков

```bash
# ключ подписи обновлений (один раз; приватный ключ — только у вас и в секретах CI)
npx tauri signer generate -w ~/.tauri/cubelaunch.key
# адрес обновлений — ваш репозиторий (в CI подставляется автоматически)
node scripts/set-update-repo.mjs owner/repo
# сборка: Windows — NSIS, macOS — .app + .dmg, Linux — AppImage + .deb + .rpm
TAURI_SIGNING_PRIVATE_KEY=~/.tauri/cubelaunch.key TAURI_SIGNING_PRIVATE_KEY_PASSWORD= npm run tauri build
```

Если генерируете свою пару ключей, замените `plugins.updater.pubkey` в `src-tauri/tauri.conf.json`
содержимым `.key.pub`. Без `TAURI_SIGNING_PRIVATE_KEY` сборка падает на этапе подписи
(`createUpdaterArtifacts`). Установщики собираются только на «своей» ОС — для всех трёх используйте CI.

Необязательные переменные сборки: `CUBELAUNCH_MSA_CLIENT_ID` (вход через Microsoft),
`CUBELAUNCH_CURSEFORGE_KEY` (CurseForge).

## Релиз

1. `npm run version:set 0.2.0` — меняет версию в `package.json`, `tauri.conf.json` и `Cargo.toml`.
2. Коммит, затем `git tag v0.2.0 && git push origin main v0.2.0`.
3. Workflow `.github/workflows/release.yml` собирает установщики на Windows, Linux и macOS (arm64 и x64),
   подписывает обновления и создаёт **черновик** релиза с `latest.json`.
4. Проверьте черновик, допишите «Что нового» (этот текст лаунчер покажет пользователям) и опубликуйте.
   До публикации лаунчеры обновление не видят.

Секреты репозитория: `TAURI_SIGNING_PRIVATE_KEY` (содержимое приватного ключа, обязательно),
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, `CUBELAUNCH_MSA_CLIENT_ID`, `CUBELAUNCH_CURSEFORGE_KEY`,
а для подписи и нотаризации macOS — `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
`APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`.
`.github/workflows/ci.yml` на каждый push гоняет clippy, тесты и сборку фронтенда на трёх ОС.

> Windows: если путь к проекту очень длинный, линкер MSVC упирается в лимит 260 символов —
> задайте короткий `CARGO_TARGET_DIR`.

## Структура

```
src/                      фронтенд (React + Vite + Tailwind 4)
  app/                    App, роутинг, тема, горячие клавиши
  components/ui/          базовые компоненты (Button, Field, Select, Switch, Menu…)
  components/layout/      оболочка окна, тосты
  features/<область>/     экраны: instances, settings, accounts…
  i18n/locales/*.json     переводы (ru — основной)
  lib/                    ipc (типизированные команды), типы, ошибки, мок-бэкенд
  stores/                 zustand-сторы
src-tauri/src/
  commands/               Tauri-команды (единственный вход из UI)
  settings/               глобальные настройки (settings.json)
  paths.rs                папка данных: портативный режим / bootstrap.json / по умолчанию
  error.rs                AppError → { kind, message, details }
  auth/ instances/ minecraft/ loaders/ java/ content/ modpacks/ downloads/
scripts/                  set-version.mjs, set-update-repo.mjs
.github/workflows/        ci.yml (проверки), release.yml (установщики и релиз)
```

## Инстансы: группы, копии, перенос

- **Группы.** Кнопка «Группа» создаёт пустую группу. Группы и инстансы перетаскиваются мышью: группа — за заголовок
  (меняет порядок), инстанс — в другую группу. Правый клик по заголовку — переименовать или удалить группу
  (инстансы останутся без группы). Порядок хранится в `groups.json` в папке данных.
- **Копирование.** «Копировать…» создаёт копию со всеми модами, конфигами и настройками; миры — по выбору.
  Логи, кэши загрузчика и время игры не копируются.
- **Экспорт инстанса** — zip с `instance.json`, `modpack.json` (если инстанс из модпака) и выбранными папками
  `.minecraft`. Файлы игры не включаются: после импорта они скачиваются с серверов Mojang.
  Путь к Java и время игры не сохраняются.
- **Импорт** (тот же диалог, что для модпаков, или перетаскивание файла в окно): zip Cubelaunch, zip инстанса
  Prism Launcher / MultiMC (`instance.cfg` + `mmc-pack.json`: версия, загрузчик, память, аргументы JVM).
  Jar-моды и посторонние компоненты Prism не переносятся — о них предупреждает предпросмотр.

## Центр загрузок

Кнопка «Загрузки» в строке состояния открывает центр загрузок: выполняющиеся задачи со скоростью и оставшимся
временем, очередь и завершённые задачи. Упавшую или отменённую задачу можно повторить. Установки, модпаки,
контент и Java идут через очередь: одновременно выполняется не больше заданного числа задач
(*Настройки → Сеть → Задач одновременно*, по умолчанию 2). Установки версий дополнительно идут по одной, так как
делят общие библиотеки и assets. История задач хранится до перезапуска лаунчера.

## Папка данных

| ОС | По умолчанию |
|---|---|
| Windows | `%APPDATA%\Cubelaunch` |
| Linux | `~/.local/share/Cubelaunch` |
| macOS | `~/Library/Application Support/Cubelaunch` |

Портативный режим: положите `portable.txt` рядом с исполняемым файлом — данные будут в `./data`.

## Как добавить язык

1. Скопируйте `src/i18n/locales/ru.json` в `<код>.json` и переведите.
2. Добавьте язык в `LANGUAGES` и `resources` в `src/i18n/index.ts`.

## Политика Mojang

Лаунчер не распространяет файлы игры: клиент, библиотеки и assets скачиваются только с официальных
серверов Mojang/Microsoft. Cubelaunch не связан с Mojang Studios и Microsoft.
