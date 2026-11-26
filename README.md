# ISOCraft

Минималистичный десктопный редактор установочных образов клиентской **Windows 11 x64** на базе **Tauri v2** (Rust + React 19 + TypeScript + Tailwind CSS v4).

ISOCraft работает по принципу **«Очередь изменений»**: настройка разделов не монтирует образ и не блокирует интерфейс на каждый клик — все операции накапливаются в памяти и применяются единым 9-шаговым асинхронным пайплайном без изменения исходного файла (`.iso`, `.wim`, `.esd`) на месте.

- **Версия:** `0.6.0`
- **Платформа:** Windows 11 x64 (UAC `requireAdministrator`)
- **Лицензия:** [MIT](LICENSE)

---

## Возможности

В интерфейсе доступны **8 рабочих разделов** и окно **Настройки**:

| Раздел | Назначение и ключевые функции |
| :--- | :--- |
| **1. Образ** (`ImageScreen`) | Открытие `.iso`, `.wim`, `.esd`; строгая проверка клиентской Windows 11 x64 (`Client`, `x64`, `build >= 22000`; блокировка `boot.wim`, Server, Win10, x86/ARM64); выбор **одной целевой редакции** с защитой очередей от случайного сброса (`ConfirmResetModal`); однократная read-only инспекция каталога с дисковым кешем; ссылки на официальный образ Microsoft и UUP dump. |
| **2. Приложения** (`AppsScreen`) | Просмотр и удаление Provisioned Appx / MSIX (с защитой системных пакетов `WindowsStore`, `DesktopAppInstaller`, `SecHealthUI`, `WebView2`, `UI.Xaml`, `VCLibs` и т.д.); добавление `.appx` / `.msix` / bundle с фильтрацией зависимостей (`x64`/`neutral`), XML-лицензией или `/SkipLicense`, `/Region:all` и `StubPackageOption`. |
| **3. Компоненты** (`FeaturesScreen`) | Включение/отключение Optional Features (включая `.NET Framework 3.5` с автоподстановкой `sources\sxs` из ISO или ручной папкой `sxs`, `/All`) и Capabilities (`/LimitAccess`). |
| **4. Обновления** (`UpdatesScreen`) | Очередь `.msu` / `.cab` пакетов и целых папок (включая цепочку **Windows 11 24H2+ Checkpoint CU → Target CU**); встроенный поиск и HTTPS-загрузка из **Microsoft Update Catalog** с проверкой подписи Authenticode; расшифровка ошибок DISM. |
| **5. Драйверы** (`DriversScreen`) | Инъекция `.inf` драйверов (файлы и папки с `/Recurse`, `/ForceUnsigned`) в `install.wim` и опционально в `boot.wim` (**Index 2 — Windows Setup**) для NVMe / Intel VMD / RAID / USB / NIC. |
| **6. Реестр** (`RegistryScreen`) | Очередь `.reg`-файлов v5 / `REGEDIT4` с изменением порядка и dry-run предпросмотром; нативное Win32-применение с правами `TrustedInstaller` (`REG_OPTION_BACKUP_RESTORE`, `NtDeleteKey`); автоматический ремаппинг `CurrentControlSet → Select\Current` и `HKCU → Users\Default\NTUSER.DAT`. |
| **7. Файлы** (`FilesScreen`) | Копирование пользовательских файлов и папок в `install.wim` (`C:\`) или корень ISO (`iso_root`) с политиками конфликтов `Overwrite` / `Skip`, быстрыми пресетами и защитой системных кустов и файлов загрузчика. |
| **8. Сборка и пресеты** (`BuildScreen`) | Экспорт/импорт JSON-пресетов (`schemaVersion: "1.0"`) с перепривязкой перемещённых путей; интерактивная сводка очередей; вывод в `ISO` или `Только install-образ` (`WIM` / `ESD`); опциональная очистка `StartComponentCleanup` (и `/ResetBase` с предупреждением); валидация `autounattend.xml`; генерация `sources\ei.cfg`; выбор `efisys_noprompt.bin` / `efisys.bin`; метка тома; 9-шаговый прогресс с таймером и экспорт диагностики. |

**Системные возможности:** безрамочный заголовок (`RU | EN`, темы **Light / Dark / System**, индикатор доступного обновления приложения на кнопке настроек), сохранение размеров окна, нижняя панель статуса с виртуальным журналом `LogDrawer`, управление кешем и загрузками в настройках, защита от закрытия при активной сборке (`CloseGuardModal`), автообнаружение и откат зависших маунтов/кустов (`workspace_state.json`), подписанные автообновления приложения (`tauri-plugin-updater`).

---

## Установка и быстрый старт

### Для пользователей

1. Скачайте последний NSIS-установщик со страницы [GitHub Releases](https://github.com/Lunqasd1337/ISOCraft/releases).
2. Установите и запустите **ISOCraft** на **Windows 11 x64** (приложение запрашивает права администратора через UAC).
3. Убедитесь, что на диске с рабочей директорией (по умолчанию `%LOCALAPPDATA%\ISOCraft\work`) есть не менее **40 ГБ** свободного места.

Подробное руководство по работе со всеми экранами — в [Руководстве пользователя](docs/USER_GUIDE.md).

### Для разработчиков

**Требования:** Windows 11 x64, Rust `stable-x86_64-pc-windows-msvc`, Visual Studio Build Tools («Desktop development with C++»), Node.js v22+ (рекомендуется v24 LTS) и `npm`.

```powershell
# 1. Загрузить и проверить по SHA-256 внешние утилиты (wimlib-imagex и MSYS2 xorriso)
.\scripts\fetch-tools.ps1

# 2. Установить зависимости фронтенда
npm install

# 3. Запустить в режиме разработки (с правами администратора)
npm run tauri dev

# 4. Собрать релизный NSIS-установщик
npm run tauri build
```

> [!NOTE]
> Для работы сборки ISO без установленного Windows ADK (`oscdimg.exe`) автоматически используется встроенный open-source `xorriso.exe`. Подробнее о сборке, подписи релизов и CI — в [Руководстве разработчика](docs/DEVELOPMENT.md).

---

## Проверки и тесты

Локальный набор проверок соответствует CI-пайплайну ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) (в CI команды `cargo check` и `cargo test --lib` запускаются с `--locked`, а локально рекомендуется добавлять `--offline` для проверки автономной сборки):

```powershell
# Проверка синхронизации версий во всех 7 файлах проекта
node scripts/sync-version.mjs --check

# Проверка паритета и полноты словарей локализации (RU / EN)
npm run check:i18n

# Юнит-тесты утилит фронтенда и скрипта версионирования
npm run test:frontend

# Проверка типов TypeScript и сборка бандла Vite
npm run build

# Проверка Rust workspace и запуск юнит-тестов без UAC
cd src-tauri
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check --workspace --locked --offline
cargo test --lib --workspace --locked --offline
```

---

## Структура репозитория

```text
├── AGENTS.md                        # Инженерные правила и критические инварианты проекта
├── README.md                        # Обзор проекта и быстрый старт
├── package.json                     # Зависимости и npm-скрипты фронтенда / Tauri CLI
├── .github/workflows/               # CI (verify) и Release (сборка NSIS + latest.json)
├── scripts/
│   ├── fetch-tools.ps1              # Загрузка и SHA-256 проверка wimlib-imagex и xorriso
│   ├── check-i18n.mjs               # Проверка RU/EN словарей (паритет, плюрализация, неиспользуемые ключи)
│   ├── test-frontend.mjs            # Юнит-тесты утилит фронтенда и версионирования
│   └── sync-version.mjs             # Синхронизация и проверка SemVer во всех файлах проекта
├── docs/
│   ├── USER_GUIDE.md                # Подробное руководство пользователя
│   ├── ARCHITECTURE.md              # Архитектура, 9-шаговый пайплайн, IPC и безопасность
│   ├── PRESET_SCHEMA.md             # Спецификация JSON-пресетов schemaVersion "1.0"
│   ├── DEVELOPMENT.md               # Сборка, тестирование, релизы и чек-лист проверки в VM
│   └── ROADMAP.md                   # Дорожная карта проекта
├── src/                             # Frontend (React 19 + TypeScript + Tailwind CSS v4 + Vite 8)
│   ├── App.tsx                      # Корневое состояние очередей, каталога, тем и сборки
│   ├── hooks/
│   │   ├── usePresetWorkflow.ts     # Экспорт/импорт пресетов и перепривязка отсутствующих путей
│   │   └── useLogStore.ts           # Потоковый store логов (батчинг и виртуальный скроллинг)
│   ├── utils/
│   │   ├── format.ts                # Локализованное форматирование размеров (B/KB/MB/GB)
│   │   ├── destinationValidation.ts # Валидация путей назначения и защита системных целей
│   │   ├── win11Version.ts          # Определение ветки Windows 11 (21H2–26H2) по билду
│   │   ├── backendErrors.ts         # Локализация сообщений, ошибок DISM/XML/реестра
│   │   └── motion.ts                # Поддержка prefers-reduced-motion
│   ├── types/studio.ts              # Общие TypeScript-контракты IPC и пресетов
│   ├── i18n/locales/{ru,en}.json    # Словари локализации (RU / EN)
│   └── components/
│       ├── Titlebar.tsx, Sidebar.tsx, SidebarIcons.tsx, StatusBar.tsx, LogDrawer.tsx
│       ├── FluentModal.tsx, FluentMotion.tsx, SettingsModal.tsx
│       ├── CloseGuardModal.tsx, ConfirmResetModal.tsx, ErrorBoundary.tsx
│       └── screens/                 # 8 экранов + build/*, AddAppxModal, CatalogModal, ScreenShell
└── src-tauri/                       # Backend (Tauri v2 + Rust)
    ├── app.manifest                 # UAC requireAdministrator, Common Controls v6, PerMonitorV2
    ├── tauri.conf.json              # Конфигурация Tauri (dev.isocraft.app, CSP, NSIS, updater)
    ├── capabilities/default.json    # Минимальный набор разрешений ACL
    ├── src/
    │   └── {lib,commands,pipeline,state}.rs  # Оболочка Tauri, IPC, состояние и пайплайн
    ├── resources/bin/               # Скачанные wimlib и xorriso + .verified-sha256.json (в .gitignore)
    └── crates/core/src/             # Тестируемое ядро (isocraft-core)
        ├── image.rs, wim.rs, iso.rs
        ├── dism.rs, apps.rs, features.rs
        ├── updates.rs, update_catalog.rs, drivers.rs, files.rs, fs_utils.rs
        ├── reg_parser.rs, registry.rs
        └── preset.rs, settings.rs, env.rs, logger.rs, process.rs
```

---

## Документация

- [Руководство пользователя](docs/USER_GUIDE.md) — пошаговое описание всех 8 разделов, настроек, восстановления и решения частых ошибок.
- [Архитектура и пайплайн](docs/ARCHITECTURE.md) — устройство модулей, 9-шаговый пайплайн, полный контракт IPC (29 команд и 5 событий) и модель безопасности.
- [Схема JSON-пресетов (`1.0`)](docs/PRESET_SCHEMA.md) — формат файлов пресета, значения по умолчанию и аудит локальных путей.
- [Руководство разработчика](docs/DEVELOPMENT.md) — настройка окружения, внешние утилиты, проверки, выпуск релизов и чек-лист приёмки в VM.
- [Дорожная карта](docs/ROADMAP.md) — дорожная карта проекта.
- [Правила и инварианты для разработки](AGENTS.md) — критические ограничения Windows/UAC/DISM/реестра/UI.

---

## Лицензия и сторонние компоненты

- Исходный код **ISOCraft** распространяется под лицензией [MIT](LICENSE) (Copyright © 2026 Lunqasd1337).
- Сторонние утилиты, загружаемые скриптом [`scripts/fetch-tools.ps1`](scripts/fetch-tools.ps1) (`wimlib-imagex`, `xorriso` и runtime-библиотеки MSYS2), распространяются под их собственными open-source лицензиями (лицензии копируются в `src-tauri/resources/bin/`).
