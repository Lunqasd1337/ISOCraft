# ISOCraft

Минималистичный десктопный редактор установочных образов клиентской **Windows 11 x64** на базе **Tauri v2** (Rust + React 19 + TypeScript + Tailwind CSS).

ISOCraft работает по принципу **«Очередь изменений»**: настройка не блокирует интерфейс на каждый клик — все операции накапливаются, а затем применяются единым 9-шаговым асинхронным пайплайном без изменения исходного файла пользователя на месте.

---

## Возможности

В интерфейсе доступны **8 разделов** и модальное окно **Настройки**:

1. **Образ (`ImageScreen`)** — открытие `.iso`, `.wim` и `.esd`; строгая валидация клиентской Windows 11 x64 (`Client`, `x64`, сборка `>= 22000`; `boot.wim`, Server, Win10 и x86/ARM64 блокируются); выбор **одной целевой редакции** (невыбранные редакции автоматически вырезаются при сборке); однократная read-only загрузка каталога с дисковым кешем; быстрые ссылки на официальную загрузку Microsoft и UUP dump; предупреждение при `< 40 ГБ` свободного места.
2. **Приложения (`AppsScreen`)** — просмотр и удаление Provisioned Appx / MSIX (с защитным предупреждением для критических пакетов `WindowsStore`, `DesktopAppInstaller`, `SecHealthUI`, `WebView2`, `UI.Xaml`, `VCLibs` и т.д.) и добавление новых `.appx` / `.msix` / bundle с фильтрацией зависимостей (`x64`/`neutral`), XML-лицензией или `/SkipLicense`, `/Region:all` и `StubPackageOption`.
3. **Компоненты (`FeaturesScreen`)** — переключение Optional Features (включая `.NET Framework 3.5` с автоподстановкой `sources\sxs` из ISO или ручным выбором папки `sxs`, `/All`) и Capabilities (`/LimitAccess`).
4. **Обновления (`UpdatesScreen`)** — интеграция `.msu` / `.cab` пакетов и целых папок с обновлениями (включая цепочку **Windows 11 24H2+ Checkpoint CU → Target CU**), расшифровка кодов ошибок DISM (`0x800f0993`, `0x80073712`, `0x800f081e`, `0x800f0823`) и опциональная очистка `StartComponentCleanup` (без `/ResetBase` по умолчанию).
5. **Драйверы (`DriversScreen`)** — инъекция `.inf` драйверов (файлы и папки с `/Recurse`, `/ForceUnsigned`) в `install.wim` и опционально в `boot.wim` (**Index 2 — Windows Setup**) для NVMe / Intel VMD / RAID / USB / NIC при сборке ISO.
6. **Реестр (`RegistryScreen`)** — очередь `.reg`-файлов с изменением порядка, dry-run предпросмотром и нативным Win32-применением с правами `TrustedInstaller` (`SeBackupPrivilege`/`SeRestorePrivilege`, `REG_OPTION_BACKUP_RESTORE`, `NtDeleteKey`), автоматическим ремаппингом `CurrentControlSet → ControlSet001` (через `Select\Current`) и `HKCU → Users\Default\NTUSER.DAT`.
7. **Файлы (`FilesScreen`)** — автоматическое копирование пользовательских файлов и папок в `install.wim` (`C:\`) или в корень ISO (`iso_root`) с политиками `Перезаписать` / `Пропустить`, быстрыми пресетами (`Windows\Setup\Scripts`, `Users\Default\Desktop`, `ProgramData`, `Tools`, `sources\$OEM$`) и защитой от перезаписи системных кустов реестра и файлов загрузчика.
8. **Сборка и пресеты (`BuildScreen`)** — экспорт и импорт JSON-пресетов (`schemaVersion: "1.0"`) с перепривязкой отсутствующих локальных путей, выбор контейнера (`ISO` / `Только install-образ`) и формата `install.*` (`WIM` / `ESD` / `SWM` с разбиением `< 4 ГБ` для FAT32), валидация и встраивание `autounattend.xml`, генерация базового `sources\ei.cfg`, выбор загрузчика UEFI (`efisys_noprompt.bin` / `efisys.bin`), метка тома, 9-шаговый чек-лист прогресса и экспорт диагностики (`dism.log` + лог приложения).

Дополнительно: кастомный безрамочный заголовок (`RU | EN`, темы **Light / Dark / System**), нижняя панель статуса с журналом сессии, защита от закрытия во время активной сборки (`CloseGuardModal`) и автообнаружение/восстановление зависшей рабочей сессии (`workspace_state.json`).

---

## Требования к окружению

- **ОС:** Windows 11 x64 (приложение запускается с правами администратора — `requireAdministrator`).
- **Rust:** `stable-x86_64-pc-windows-msvc` + Visual Studio Build Tools («Desktop development with C++»).
- **Node.js:** v20+ и `npm`.
- **WebView2 Runtime:** встроен в Windows 11.
- **Системные утилиты Windows:** `dism.exe` и `powershell.exe` (модули `Dism` и `Storage`).
- **Опционально для сборки ISO:** Windows ADK (`oscdimg.exe`). Если не установлен, автоматически используется open-source `xorriso.exe`.

---

## Быстрый старт и сборка

### 1. Скачивание внешних open-source утилит
Внешние бинарники не хранятся в git (`src-tauri/resources/bin/` в `.gitignore`). Перед первым запуском загрузите и проверьте по SHA-256 `wimlib-imagex` и MSYS2 `xorriso` (+ runtime DLL и лицензии):

```powershell
.\scripts\fetch-tools.ps1
```

Для принудительной перезагрузки используйте `-Force`:

```powershell
.\scripts\fetch-tools.ps1 -Force
```

### 2. Установка JS-зависимостей

```powershell
npm install
```

### 3. Запуск в режиме разработки

```powershell
npm run tauri dev
```

### 4. Релизная сборка установщика (NSIS)

```powershell
npm run tauri build
```

Для подписанных обновлений через `tauri-plugin-updater` используется приватный ключ подписи (`TAURI_SIGNING_PRIVATE_KEY` и опционально `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). Готовый workflow [.github/workflows/release.yml](.github/workflows/release.yml) автоматически скачивает внешние утилиты, собирает NSIS-установщик и публикует подписанный GitHub Release с `latest.json` при пуше тега вида `v*` (например, `v0.2.1`).

---

## Обновления и лицензия

- Проверка и установка подписанных обновлений встроены в окно **Настройки** (`tauri-plugin-updater` → `https://github.com/Lunqasd1337/ISOCraft/releases/latest/download/latest.json`).
- Исходный код ISOCraft распространяется под лицензией [MIT](LICENSE). Сторонние утилиты, загружаемые скриптом `scripts/fetch-tools.ps1` (`wimlib-imagex`, `xorriso` и runtime-библиотеки MSYS2), распространяются под их собственными open-source лицензиями.

---

## Проверки и тесты

```powershell
# Проверка TypeScript и бандла Vite
npm run build

# Проверка всего Rust workspace
cd src-tauri
cargo check --workspace --offline

# Запуск юнит-тестов ядра (без требования UAC в тестовом окружении)
cargo test -p isocraft-core --offline
```

---

## Структура репозитория

```text
├── AGENTS.md                       # Инженерные правила и инварианты проекта
├── README.md                       # Обзор проекта, установка и команды сборки
├── package.json                    # Зависимости и npm-скрипты фронтенда / Tauri CLI
├── scripts/
│   └── fetch-tools.ps1             # Загрузка и SHA-256 проверка wimlib-imagex и xorriso
├── docs/
│   ├── ARCHITECTURE.md             # Архитектура, 9-шаговый пайплайн, IPC и чек-лист тестов
│   └── PRESET_SCHEMA.md            # Спецификация JSON-пресетов schemaVersion "1.0"
├── src/                            # Frontend (React 19 + TypeScript + Tailwind CSS)
│   ├── App.tsx                     # Состояние очередей, пресетов, темы и сборки
│   ├── types/studio.ts             # Общие TS-контракты (IPC и пресеты)
│   ├── i18n/locales/{ru,en}.json   # Словари локализации (RU / EN)
│   └── components/
│       ├── Titlebar.tsx, Sidebar.tsx, StatusBar.tsx, LogDrawer.tsx
│       ├── SettingsModal.tsx, CloseGuardModal.tsx
│       └── screens/                # 8 экранов приложения + AddAppxModal
└── src-tauri/                      # Backend (Tauri v2 + Rust)
    ├── app.manifest                # UAC requireAdministrator, Common Controls v6, PerMonitorV2
    ├── tauri.conf.json             # Конфиг Tauri (dev.isocraft.app, NSIS, resources)
    ├── src/lib.rs                  # IPC-команды Tauri и оркестратор start_build
    ├── resources/bin/              # Скачанные wimlib и xorriso (игнорируется git)
    └── crates/core/src/            # Чистое тестируемое ядро (isocraft-core)
        ├── image.rs, wim.rs, iso.rs
        ├── dism.rs, apps.rs, features.rs
        ├── updates.rs, drivers.rs, files.rs
        ├── reg_parser.rs, registry.rs
        ├── preset.rs, settings.rs, env.rs, logger.rs, process.rs
```

Дополнительная техническая документация:
- [Архитектура и 9-шаговый пайплайн](docs/ARCHITECTURE.md)
- [Схема JSON-пресетов v1.0](docs/PRESET_SCHEMA.md)
- [Инструкции и инварианты для разработки](AGENTS.md)
