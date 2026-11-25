# AGENTS.md — Инструкции и инварианты разработки ISOCraft

Этот документ описывает реальную архитектуру и критические технические правила проекта **ISOCraft** (Tauri v2 / Rust + React 19 + TypeScript + Tailwind CSS) для разработчиков и ИИ-агентов.

Дополнительные документы: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md), [`docs/PRESET_SCHEMA.md`](docs/PRESET_SCHEMA.md), [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) и [`docs/USER_GUIDE.md`](docs/USER_GUIDE.md).

---

## 1. Структура Rust Workspace

Бэкенд намеренно разделён на два крейта в [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml):
1. `isocraft-core` ([`src-tauri/crates/core`](src-tauri/crates/core/Cargo.toml)) — вся бизнес-логика без зависимостей от GUI, `tauri-plugin-dialog` и `rfd`. Благодаря этому юнит-тесты можно запускать без прав администратора (UAC) и без инициализации Common Controls v6 (`TaskDialogIndirect`):
   ```powershell
   cargo test -p isocraft-core --locked --offline
   ```
2. `isocraft` ([`src-tauri/src/`](src-tauri/src/lib.rs)) — оболочка Tauri v2 (`dev.isocraft.app`), разделённая на `lib.rs` (настройка WebView2, геометрия окна, перехват `CloseRequested`, регистрация 29 IPC-команд), `commands.rs` (IPC-команды), `pipeline.rs` (`preflight_validate_build_config` и оркестратор `run_build_pipeline`) и `state.rs` (`AppState`, эмиттеры, `cleanup_session_dir_if_clean`) + встроенный Windows-манифест.

**Важно:** не добавляйте внешние Cargo-зависимости без необходимости — проект должен собираться с `--locked --offline` на основе зафиксированного `Cargo.lock`.

---

## 2. Критические технические правила (НЕ НАРУШАТЬ)

### 2.1. UAC, WebView2, ACL и Windows UIPI
- Приложение собирается с манифестом [`src-tauri/app.manifest`](src-tauri/app.manifest): `requireAdministrator`, `Microsoft.Windows.Common-Controls` v6, Windows 10/11 `<supportedOS>`, `PerMonitorV2` DPI, строго под `x64`.
- Для WebView2 явно задаётся папка данных `%LOCALAPPDATA%\ISOCraft\WebView2` (`webview_user_data_dir()`), чтобы браузерный движок корректно работал под повышенными правами администратора.
- Из-за Windows UIPI обычный Проводник не может перетаскивать файлы (drag-and-drop) в окно с повышенными правами. Выбор всех файлов и папок выполняется строго через нативные диалоги `@tauri-apps/plugin-dialog`.
- Не ослабляйте минимальный ACL в [`src-tauri/capabilities/default.json`](src-tauri/capabilities/default.json) и строгий CSP в [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) (никаких `core:default`, `desktop.json` или `unsafe-eval`).
- Не удаляйте перехват `WindowEvent::CloseRequested` в [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs): при `process_guard.is_busy()` окно обязано запросить подтверждение (`CloseGuardModal`) и при принудительном закрытии убить дерево дочерних процессов (`kill_all_tracked_trees`) + откатить монтирования (`recover_workspace`).
- Все дочерние процессы должны запускаться через [`process.rs`](src-tauri/crates/core/src/process.rs) (`ProcessGuard` с Windows Job Object `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, `CREATE_NO_WINDOW` и принудительным `kill_process_tree`), чтобы гарантировать завершение всех потомков при отмене или падении.
- На Windows вызов Tauri `app.path().resource_dir()` возвращает verbatim-путь с префиксом `\\?\`, поэтому [`env.rs`](src-tauri/crates/core/src/env.rs) снимает префиксы `\\?\` / `\\?\UNC\` через `strip_windows_verbatim_prefix` (для стандартных путей короче 260 UTF-16 символов) и собирает относительные подпути утилит посегментно с нативным разделителем — не обходите `env.rs` и не передавайте сырой `resource_dir()` напрямую в UI или CLI.

### 2.2. Строгая валидация образов, путей и изоляция исходников
- Разрешена только **клиентская Windows 11 x64** (`Architecture == "x64"`, `InstallationType == "Client"`, `build >= 22000`). Образы `boot.wim` / WinPE, Windows Server, Windows 10 и `x86` / `ARM64` блокируются в [`image.rs`](src-tauri/crates/core/src/image.rs).
- Исходный файл пользователя (`.iso` / `.wim` / `.esd`) **никогда не модифицируется на месте**.
- При выборе редакции через `wimlib-imagex export` экспортируется **только выбранный индекс** в одноиндексный рабочий `install.wim` (Index `1`). Это одновременно отрезает лишние редакции и позволяет DISM работать с источниками из `install.esd`.
- После извлечения файлов из примонтированного через `Mount-DiskImage` ISO всегда вызывается `clear_readonly_recursive(&iso_root)`, иначе DISM, `ei.cfg` и перезапись `sources\install.*` завершатся ошибкой `Access Denied`.
- Чтение каталога образа (`load_image_catalog` в [`dism.rs`](src-tauri/crates/core/src/dism.rs)) использует дисковый кеш `%LOCALAPPDATA%\ISOCraft\work\cache\catalog_*.json`; при промахе выполняет однократный **read-only** маунт, читает JSON через PowerShell и немедленно размонтирует с `/Discard`.
- Входные и выходные пути, а также `workspaceDir` (проверяется через `validate_workspace_dir` в [`settings.rs`](src-tauri/crates/core/src/settings.rs)) не должны быть UNC/device-путями, симлинками/reparse-точками или находиться внутри временной папки `session_current`.

### 2.3. DISM, PowerShell и сеть
- Никогда не парсите локализованный консольный текст `dism.exe`. Всегда передавайте `/English` и `/LogPath:<workspace>\dism.log`.
- Структурированные списки (Appx, Features, Capabilities) запрашивайте через PowerShell-модуль `Dism` + `ConvertTo-Json`, оборачивая коллекции в `@(...)` и вызывая `.ToString()` на перечислениях.
- Все вызовы `run_powershell_script` принудительно выставляют UTF-8 без BOM (`[Console]::OutputEncoding` и `$OutputEncoding`), чтобы корректно передавать кириллические пути.
- В оффлайн-очереди `StartComponentCleanup` выполняется **без `/ResetBase` по умолчанию**. Если оффлайн-образ уже имеет отложенные операции CBS (`0x800f0806`, например после включения компонентов или обновлений), очистка должна пропускаться с предупреждением в логе (`Ok(false)` в [`updates.rs`](src-tauri/crates/core/src/updates.rs)), не прерывая сборку.
- Пользовательский `oscdimg.exe` из настроек обязан проходить `is_trusted_oscdimg_path` в [`env.rs`](src-tauri/crates/core/src/env.rs) (подпись Microsoft Authenticode через `WinVerifyTrust` и расположение в защищённом системном каталоге).
- Загрузки из Microsoft Update Catalog ([`update_catalog.rs`](src-tauri/crates/core/src/update_catalog.rs)) разрешены только по `https://` с хостов `*.microsoft.com` / `*.windowsupdate.com`, с проверкой Authenticode на скачанных `.msu`/`.cab`. Внешний браузер (`open_external_url`) ограничен списком `ALLOWED_URLS`.

### 2.4. Нативный оффлайн-реестр (`.reg`)
- Никогда не вызывайте `reg.exe import` на пользовательском `.reg`-файле (риск перезаписи живого реестра хоста).
- В [`registry.rs`](src-tauri/crates/core/src/registry.rs) используются прямые Win32 API:
  - активация `SeBackupPrivilege` и `SeRestorePrivilege` (`AdjustTokenPrivileges`);
  - загрузка кустов через `RegLoadKeyW` под уникальными именами `ISOCRAFT_<pid>_<HIVE>` с предварительной записью в `workspace_state.json`;
  - открытие/создание ключей с `REG_OPTION_BACKUP_RESTORE` (обход защиты `TrustedInstaller`);
  - удаление ключей `[-Key]` через `RegOpenKeyExW(REG_OPTION_BACKUP_RESTORE | DELETE | KEY_ENUMERATE_SUB_KEYS)` + `ntdll!NtDeleteKey`.
- В [`reg_parser.rs`](src-tauri/crates/core/src/reg_parser.rs) и [`registry.rs`](src-tauri/crates/core/src/registry.rs): `HKLM\SYSTEM\CurrentControlSet` перенаправляется через `Select\Current` (`ControlSet001` и т.п.), `HKCU` → `Users\Default\NTUSER.DAT`, `HKCR` → `SOFTWARE\Classes`, `HKU\.DEFAULT` → `config\DEFAULT`. Неподдерживаемые ветки (`HKLM\HARDWARE`, `HKCC`, сторонние `HKU\<SID>`) помечаются предупреждением и пропускаются; значения типа `REG_LINK` (`hex(6)`) и удаление защищённых корней кустов (`is_protected_root_delete`: корни кустов, `SOFTWARE\Classes`, `SYSTEM\Select`, `SYSTEM\ControlSetNNN`) запрещены.
- **Критически важно:** все `HKEY` (RAII `OwnedHKey`) закрываются, кусты сбрасываются `RegFlushKey` и выгружаются `RegUnLoadKeyW` **строго до** `DISM /Unmount-Image /Commit` или `/Discard`.

### 2.5. Безопасность пользовательских файлов (`files.rs`)
- Все пути назначения нормализуются в [`files.rs`](src-tauri/crates/core/src/files.rs): запрещены `..`, буквы дисков (`:`), UNC-пути, зарезервированные имена Windows (`CON`, `PRN`, `AUX`, `NUL`, `COM1..9`, `LPT1..9`) и спецсимволы; при сканировании и копировании симлинки пропускаются, а `check_target_no_reparse_escape` пресекает проход через NTFS-junctions внутри смонтированного образа (например, `Users\All Users`).
- Для каждого копируемого файла (включая вложенные файлы внутри папок) проверяется запрет на перезапись критических путей: кустов `Windows\System32\config*` и `Users\Default\NTUSER.DAT` в `install.wim`, а также `sources\install.wim/esd/*.swm`, `sources\boot.wim`, `boot\etfsboot.com`, `efi\microsoft\boot\efisys*.bin` и `bootmgr*` в `iso_root`.

### 2.6. Сборка ISO (`oscdimg.exe` и `xorriso`)
- При наличии доверенного Windows ADK (автопоиск или путь из Настроек) используется `oscdimg.exe` (`-m -o -u2 -udfver102`).
- Встроенный open-source fallback — MSYS2 `xorriso.exe -as mkisofs` ([`iso.rs`](src-tauri/crates/core/src/iso.rs)): пути автоматически переводятся в `/cygdrive/<drive>/...`, используется dual El Torito (`boot/etfsboot.com` + `efi/microsoft/boot/efisys_noprompt.bin` / `efisys.bin`) и `-isohybrid-gpt-basdat`. Флаг `-udf` в `xorriso -as mkisofs` **не применяется** (отвергается этой версией `xorriso`), используется `-iso-level 3 -J -joliet-long`.

### 2.7. Интерфейс, темы и локализация
- **Никакого хардкода текстов в JSX:** все строки интерфейса хранятся в [`src/i18n/locales/ru.json`](src/i18n/locales/ru.json) и [`src/i18n/locales/en.json`](src/i18n/locales/en.json) с точным совпадением ключей, плюрализаций и `{{placeholder}}` (проверяется `npm run check:i18n`).
- **Tailwind CSS v4 и токены темы:** стили настраиваются через `@tailwindcss/vite` и CSS-директивы (`@import "tailwindcss"`, `@custom-variant dark`, `@theme inline`, `@layer base`) в [`src/index.css`](src/index.css) без отдельного `tailwind.config.js`. Токены `subtle`, `border-subtle`, `status-*-bg`, `switch-off-*` заданы как готовые цвета (`rgba`/`hex`), поэтому не применяйте к ним `/opacity`-модификаторы (модификаторы вида `bg-accent/10`, `bg-surface/80` используйте только для семантических токенов вроде `canvas`, `sidebar`, `surface`, `text-primary`, `text-secondary`, `accent`, `status-danger`).

---

## 3. Команды проверки перед коммитом

```powershell
# 1. Версия и фронтенд: паритет версий, локализация, тесты и сборка Vite
node scripts/sync-version.mjs --check
npm run check:i18n
npm run test:frontend
npm run build

# 2. Rust: форматирование, clippy, проверка workspace и тесты
cd src-tauri
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check --workspace --locked --offline
cargo test --lib --workspace --locked --offline
```
