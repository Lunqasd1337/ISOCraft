# Руководство разработчика ISOCraft

Документ описывает настройку окружения разработки, работу со скриптами репозитория, локальные проверки качества (соответствующие CI), процесс выпуска релизов и процедуру ручной приёмки в виртуальной машине.

---

## Содержание

- [1. Системные требования и инструменты](#1-системные-требования-и-инструменты)
- [2. Подготовка репозитория и внешние утилиты](#2-подготовка-репозитория-и-внешние-утилиты)
- [3. Локальный запуск и сборка](#3-локальный-запуск-и-сборка)
- [4. Проверки качества и тесты](#4-проверки-качества-и-тесты)
- [5. Версионирование и выпуск релизов](#5-версионирование-и-выпуск-релизов)
- [6. Чек-лист ручной проверки в VM](#6-чек-лист-ручной-проверки-в-vm)

---

## 1. Системные требования и инструменты

- **ОС:** Windows 11 x64.
- **Rust:** toolchain `stable-x86_64-pc-windows-msvc` (edition `2024`) с компонентами `rustfmt` и `clippy`.
- **C++ Build Tools:** Visual Studio Build Tools с рабочей нагрузкой «Desktop development with C++» (требуется для линковки Win32/Tauri на Windows).
- **Node.js:** v22+ (рекомендуется v24 LTS) и `npm`.
- **WebView2 Runtime:** встроен в Windows 11.
- **Системные утилиты Windows:** `dism.exe`, `curl.exe`, `powershell.exe` (модули `Dism` и `Storage`), `tar.exe` (встроен в `System32`, нужен скрипту `fetch-tools.ps1` для распаковки `.zip` и `.tar.zst`).
- **Опционально:** Windows ADK (Deployment Tools — `oscdimg.exe`) для тестирования сборки ISO через ADK.

> [!IMPORTANT]
> Приложение собирается с манифестом `requireAdministrator`. Юнит-тесты ядра `isocraft-core` запускаются **без прав администратора**, однако для запуска самого GUI (`npm run tauri dev`) терминал или подтверждение UAC потребуют повышенных привилегий.

---

## 2. Подготовка репозитория и внешние утилиты

Внешние бинарники не хранятся в git (`src-tauri/resources/bin/` добавлен в `.gitignore`). За их загрузку и верификацию отвечает скрипт [`scripts/fetch-tools.ps1`](../scripts/fetch-tools.ps1):

```powershell
# Загрузка и проверка SHA-256 всех архивов и извлечённых файлов
.\scripts\fetch-tools.ps1

# Принудительная перезагрузка поверх существующих файлов
.\scripts\fetch-tools.ps1 -Force
```

Что устанавливает скрипт:
1. **`wimlib-imagex` (`1.14.5`, x86_64)** → `src-tauri/resources/bin/wimlib/` (`wimlib-imagex.exe`, `libwim-15.dll` и файлы лицензий `COPYING*`).
2. **MSYS2 `xorriso` (`1.5.8.2`, x86_64)** + runtime-зависимости (`msys2-runtime`, `libiconv`, `zlib`, `libbz2`, `libreadline`, `ncurses`) → `src-tauri/resources/bin/xorriso/` (`xorriso.exe`, `msys-*.dll` и `licenses/`).
3. **Манифест целостности** → `src-tauri/resources/bin/.verified-sha256.json` (фиксирует SHA-256 скачанных архивов и всех `.exe`/`.dll`; при повторном вызове без `-Force` скрипт проверяет манифест и пропускает загрузку, если хеши совпадают).

---

## 3. Локальный запуск и сборка

```powershell
# Установка JS-зависимостей
npm install

# Запуск только Vite-сервера фронтенда (http://localhost:1420; без нативного бэкенда; для вёрстки UI)
npm run dev

# Полноценный запуск Tauri v2 в режиме разработки (с поддержкой HMR на 1421 при TAURI_DEV_HOST)
npm run tauri dev

# Релизная сборка NSIS-установщика (perMachine) и updater-артефактов
npm run tauri build

# Локальная сборка только бинарника без упаковки и подписи updater-артефактов
npm run tauri build -- --no-bundle
```

> [!NOTE]
> Для сборки артефактов обновления (`createUpdaterArtifacts: true` в [`src-tauri/tauri.conf.json`](../src-tauri/tauri.conf.json)) в окружении релизной сборки требуются переменные `TAURI_SIGNING_PRIVATE_KEY` и (опционально) `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Локально при отсутствии ключа `npm run tauri build` выдаст ошибку подписи updater-пакета, если не используется `--no-bundle` или тестовый ключ.

---

## 4. Проверки качества и тесты

Все проверки автоматически запускаются в GitHub Actions ([`.github/workflows/ci.yml`](../.github/workflows/ci.yml)) на раннере `windows-latest` при каждом `push` и `pull_request` в `main`/`master` (для Node-зависимостей используется `npm ci`):

| Шаг | Команда | Что проверяет |
| :--- | :--- | :--- |
| Версии | `node scripts/sync-version.mjs --check` | Совпадение версии во всех 7 файлах проекта с `package.json`. |
| Словари i18n | `npm run check:i18n` | Скрипт [`scripts/check-i18n.mjs`](../scripts/check-i18n.mjs): паритет базовых ключей `ru.json` / `en.json`, полноту плюрализаций (RU: `one`/`few`/`many`; EN: `one`/`other`), совпадение `{{placeholder}}`, отсутствие пропущенных ключей в `src/**/*.{ts,tsx}` и неиспользуемых ключей (с учётом динамических префиксов `screens.build.missingKinds.` и `screens.build.steps.`). |
| Юнит-тесты фронтенда | `npm run test:frontend` | Скрипт [`scripts/test-frontend.mjs`](../scripts/test-frontend.mjs): собирает `destinationValidation`, `win11Version`, `format`, `backendErrors`, `i18n` через `esbuild` в Node и тестирует их вместе с `sync-version.mjs`. |
| Типы и бандл | `npm run build` | Последовательно запускает `check:i18n`, `tsc --noEmit` и `vite build`. |
| Форматирование Rust | `cd src-tauri; cargo fmt --all -- --check` | Соответствие кода стилю `rustfmt`. |
| Линтер Rust | `cd src-tauri; cargo clippy --workspace --all-targets --locked -- -D warnings` | Отсутствие предупреждений Clippy во всех таргетах workspace. |
| Проверка сборки Rust | `cd src-tauri; cargo check --workspace --locked --offline` | Компиляция `isocraft-core` и `isocraft` без обновления `Cargo.lock` и без сети (в CI запускается с `--locked`). |
| Юнит-тесты Rust | `cd src-tauri; cargo test --lib --workspace --locked --offline` | Все библиотечные тесты `isocraft-core` и `isocraft` (`pipeline::tests`, `preset::tests`, `reg_parser::tests`, `files::tests`, `settings::tests` и др.) без UAC (в CI запускается с `--locked`). |

---

## 5. Версионирование и выпуск релизов

### Синхронизация версий (`scripts/sync-version.mjs`)

Версия проекта синхронизируется одновременно в **7 файлах**:
1. `package.json`
2. `package-lock.json` (`version` и `packages[""].version`)
3. `src-tauri/tauri.conf.json`
4. `src-tauri/Cargo.toml`
5. `src-tauri/crates/core/Cargo.toml`
6. `src-tauri/Cargo.lock` (записи пакетов `isocraft` и `isocraft-core`)
7. `src-tauri/app.manifest` (`<assemblyIdentity version="X.Y.Z.0" .../>`)

Основные режимы запуска:
```powershell
# Проверить соответствие всех 7 файлов текущей версии из package.json (или указанной)
node scripts/sync-version.mjs --check [version]

# Обновить все 7 файлов до указанной SemVer-версии
node scripts/sync-version.mjs 0.5.1

# Вычислить следующую версию по тегам git (используется в release.yml)
node scripts/sync-version.mjs --resolve --git-tags --bump patch --prerelease auto
```

### Автоматизированный релиз (`.github/workflows/release.yml`)

Релизный пайплайн выполняется в GitHub Actions окружении **`environment: release`** и работает в двух режимах:

1. **Ручной запуск (`Actions → Release → Run workflow`)** строго из веток `main` или `master`:
   - Вычисляет целевую версию (или берёт переданную вручную `version`, например `0.5.1` или `0.6.0-rc.1`) и проверяет, что тег `vX.Y.Z` ещё не существует.
   - Обновляет и проверяет все 7 файлов версии через `sync-version.mjs`.
   - Прогоняет полный набор проверок (`check:i18n`, `test:frontend`, `build`, `cargo fmt`, `cargo clippy`, `cargo check`, `cargo test --lib`).
   - Загружает и верифицирует `wimlib-imagex` + `xorriso` через `.\scripts\fetch-tools.ps1`.
   - При включённом `commit_version` создаёт коммит `chore(release): vX.Y.Z [skip ci]`, ставит аннотированный тег `vX.Y.Z` и пушит его.
   - Собирает NSIS-установщик и подписывает артефакты (`tauri-apps/tauri-action` с `TAURI_SIGNING_PRIVATE_KEY` и `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`), формируя GitHub Release (по умолчанию Draft, с `generateReleaseNotes: true` и `updaterJsonPreferNsis: true`) с `latest.json` для автообновлений.
2. **Push тега `v*`** (например, `v0.5.1`): проверяет соответствие версии тега всем 7 файлам, прогоняет проверки и создаёт GitHub Release в статусе **Draft** (`releaseDraft: true`).

---

## 6. Чек-лист ручной проверки в VM

Для полной интеграционной приёмки на реальном образе клиентской Windows 11 x64 (`23H2` / `24H2+`) перед крупным релизом:

1. **Инспекция и одноредакционный экспорт:** открыть многоредакционный ISO Windows 11 x64, выбрать одну редакцию и убедиться, что в итоговом `sources\install.wim` / `install.esd` остался ровно 1 индекс (`Index 1`).
2. **Приложения и компоненты:** проверить удаление Provisioned Appx и добавление `.msix`/`.appxbundle` с `x64`/`neutral` зависимостями, а также включение `NetFx3` из `sources\sxs` и из вручную указанной папки `sxs`.
3. **Обновления 24H2+:** интегрировать папку с парой *Checkpoint CU + Target LCU* и проверить загрузку пакета из встроенного Microsoft Update Catalog.
4. **Драйверы:** выполнить инъекцию `.inf` в `install.wim` и `boot.wim` (**Index 2**), убедиться, что установщик Windows в VM видит контроллер/диск.
5. **Оффлайн-реестр (`.reg`):** применить `.reg`-файл, затрагивающий `HKCU` (`Users\Default\NTUSER.DAT`), `HKLM\SYSTEM\CurrentControlSet` (`ControlSet001`) и удаление защищённого `TrustedInstaller`-ключа (`[-Key]`), убедиться, что кусты корректно выгружены до `Unmount-Image /Commit`.
6. **Пользовательские файлы:** проверить копирование в `install.wim` (например, `Windows\Setup\Scripts\SetupComplete.cmd`) и в корень ISO (`iso_root`), а также блокировку системных путей.
7. **Пресеты и перепривязка:** сохранить JSON-пресет, переместить один из локальных файлов, загрузить пресет и проверить карточку **«Отсутствующие пути»** с действием **«Указать путь...»**.
8. **Загрузка в VM и установка:** проверить загрузку и установку собранного ISO в Hyper-V / VMware / QEMU:
   - в режиме **UEFI** (`efisys_noprompt.bin`);
   - в режиме **Legacy BIOS** (`etfsboot.com`);
   - со сжатием `install.esd` и файлом ответов `autounattend.xml` (при проверке `xorriso` и `oscdimg`).
9. **Отмена и аварийное восстановление:** прервать активную сборку кнопкой отмены (и отдельно проверить `CloseGuardModal` при попытке закрыть окно), убедиться в отсутствии висящих процессов `dism.exe`/`wimlib-imagex.exe` и в чистом откате `workspace_state.json`.
