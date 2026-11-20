# AGENTS.md — Инструкции и инварианты разработки ISOCraft

Этот документ описывает реальную архитектуру и критические технические правила проекта **ISOCraft** (Tauri v2 / Rust + React 19 + TypeScript + Tailwind CSS) для разработчиков и ИИ-агентов.

Детали пайплайна, IPC-команд и пресетов вынесены в [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) и [`docs/PRESET_SCHEMA.md`](docs/PRESET_SCHEMA.md).

---

## 1. Структура Rust Workspace

Бэкенд намеренно разделён на два крейта в [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml):
1. `isocraft-core` ([`src-tauri/crates/core`](src-tauri/crates/core/Cargo.toml)) — вся бизнес-логика без зависимостей от GUI, `tauri-plugin-dialog` и `rfd`. Благодаря этому юнит-тесты можно запускать без прав администратора (UAC) и без инициализации Common Controls v6 (`TaskDialogIndirect`):
   ```powershell
   cargo test -p isocraft-core --offline
   ```
2. `isocraft` ([`src-tauri/src/lib.rs`](src-tauri/src/lib.rs)) — оболочка Tauri v2 (`dev.isocraft.app`), команды IPC, оркестратор `start_build` и встроенный Windows-манифест.

**Важно:** не добавляйте внешние Cargo-зависимости без необходимости — проект должен собираться с `--offline` на основе зафиксированного `Cargo.lock`.

---

## 2. Критические технические правила (НЕ НАРУШАТЬ)

### 2.1. UAC, WebView2 и Windows UIPI
- Приложение собирается с манифестом [`src-tauri/app.manifest`](src-tauri/app.manifest): `requireAdministrator`, `Microsoft.Windows.Common-Controls` v6, Windows 10/11 `<supportedOS>`, `PerMonitorV2` DPI, строго под `x64`.
- Для WebView2 явно задаётся папка данных `%LOCALAPPDATA%\ISOCraft\WebView2` (`webview_user_data_dir()`), чтобы браузерный движок корректно работал под повышенными правами администратора.
- Из-за Windows UIPI обычный Проводник не может перетаскивать файлы (drag-and-drop) в окно с повышенными правами. Выбор всех файлов и папок выполняется строго через нативные диалоги `@tauri-apps/plugin-dialog`.
- Не удаляйте перехват `WindowEvent::CloseRequested` в [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs): при `process_guard.is_busy()` окно обязано запросить подтверждение (`CloseGuardModal`) и при принудительном закрытии убить дерево дочерних процессов (`kill_all_tracked_trees`) + откатить монтирования (`recover_workspace`).

### 2.2. Строгая валидация образов и изоляция исходников
- Разрешена только **клиентская Windows 11 x64** (`Architecture == "x64"`, `InstallationType == "Client"`, `build >= 22000`). Образы `boot.wim` / WinPE, Windows Server, Windows 10 и `x86` / `ARM64` блокируются в [`image.rs`](src-tauri/crates/core/src/image.rs).
- Исходный файл пользователя (`.iso` / `.wim` / `.esd`) **никогда не модифицируется на месте**.
- При выборе редакции через `wimlib-imagex export` экспортируется **только выбранный индекс** в одноиндексный рабочий `install.wim` (Index `1`). Это одновременно отрезает лишние редакции и позволяет DISM работать с источниками из `install.esd`.
- После извлечения файлов из примонтированного через `Mount-DiskImage` ISO всегда вызывается `clear_readonly_recursive(&iso_root)`, иначе DISM, `ei.cfg` и перезапись `sources\install.*` завершатся ошибкой `Access Denied`.
- Чтение каталога образа (`load_image_catalog` в [`dism.rs`](src-tauri/crates/core/src/dism.rs)) использует дисковый кеш `%LOCALAPPDATA%\ISOCraft\work\cache\catalog_*.json`; при промахе выполняет однократный **read-only** маунт, читает JSON через PowerShell и немедленно размонтирует с `/Discard`.

### 2.3. DISM и PowerShell
- Никогда не парсите локализованный консольный текст `dism.exe`. Всегда передавайте `/English` и `/LogPath:<workspace>\dism.log`.
- Структурированные списки (Appx, Features, Capabilities) запрашивайте через PowerShell-модуль `Dism` + `ConvertTo-Json`, оборачивая коллекции в `@(...)` и вызывая `.ToString()` на перечислениях.
- Все вызовы `run_powershell_script` принудительно выставляют UTF-8 без BOM (`[Console]::OutputEncoding` и `$OutputEncoding`), чтобы корректно передавать кириллические пути.
- В оффлайн-очереди `StartComponentCleanup` выполняется **без `/ResetBase` по умолчанию**. Если оффлайн-образ уже имеет отложенные операции CBS (`0x800f0806` / `-2146498554`, например после включения компонентов или обновлений), очистка должна пропускаться с предупреждением в логе (`Ok(false)` в [`updates.rs`](src-tauri/crates/core/src/updates.rs)), не прерывая сборку.

### 2.4. Нативный оффлайн-реестр (`.reg`)
- Никогда не вызывайте `reg.exe import` на пользовательском `.reg`-файле (риск перезаписи живого реестра хоста).
- В [`registry.rs`](src-tauri/crates/core/src/registry.rs) используются прямые Win32 API:
  - активация `SeBackupPrivilege` и `SeRestorePrivilege` (`AdjustTokenPrivileges`);
  - загрузка кустов через `RegLoadKeyW` под уникальными именами `ISOCRAFT_<pid>_<HIVE>` с предварительной записью в `workspace_state.json`;
  - открытие/создание ключей с `REG_OPTION_BACKUP_RESTORE` (обход защиты `TrustedInstaller`);
  - удаление ключей `[-Key]` через `RegOpenKeyExW(REG_OPTION_BACKUP_RESTORE | DELETE | KEY_ENUMERATE_SUB_KEYS)` + `ntdll!NtDeleteKey`.
- В [`reg_parser.rs`](src-tauri/crates/core/src/reg_parser.rs): `HKLM\SYSTEM\CurrentControlSet` перенаправляется через `Select\Current` (`ControlSet001` и т.п.), `HKCU` → `Users\Default\NTUSER.DAT`, `HKCR` → `SOFTWARE\Classes`, `HKU\.DEFAULT` → `config\DEFAULT`. Неподдерживаемые ветки (`HKLM\HARDWARE`, `HKCC`, сторонние `HKU\<SID>`) помечаются предупреждением и пропускаются.
- **Критически важно:** все `HKEY` (RAII `OwnedHKey`) закрываются, кусты сбрасываются `RegFlushKey` и выгружаются `RegUnLoadKeyW` **строго до** `DISM /Unmount-Image /Commit` или `/Discard`.

### 2.5. Безопасность пользовательских файлов (`files.rs`)
- Все пути назначения нормализуются в [`files.rs`](src-tauri/crates/core/src/files.rs): запрещены `..`, буквы дисков (`:`), UNC-пути и спецсимволы; при сканировании и копировании симлинки пропускаются.
- Для каждого копируемого файла (включая вложенные файлы внутри папок) проверяется запрет на перезапись критических путей: кустов `Windows\System32\config*` и `Users\Default\NTUSER.DAT` в `install.wim`, а также `sources\install.wim/esd/*.swm`, `sources\boot.wim`, `boot\etfsboot.com`, `efi\microsoft\boot\efisys*.bin` и `bootmgr*` в `iso_root`.

### 2.6. Сборка ISO (`oscdimg.exe` и `xorriso`)
- При наличии Windows ADK (автопоиск или путь из Настроек) используется `oscdimg.exe` (`-m -o -u2 -udfver102`).
- Встроенный open-source fallback — MSYS2 `xorriso.exe -as mkisofs` ([`iso.rs`](src-tauri/crates/core/src/iso.rs)): пути автоматически переводятся в `/cygdrive/<drive>/...`, используется dual El Torito (`boot/etfsboot.com` + `efi/microsoft/boot/efisys_noprompt.bin` / `efisys.bin`) и `-isohybrid-gpt-basdat`. Флаг `-udf` в `xorriso -as mkisofs` **не применяется** (отвергается этой версией `xorriso`), используется `-iso-level 3 -J -joliet-long`.

### 2.7. Интерфейс, темы и локализация
- **Никакого хардкода текстов в JSX:** все строки интерфейса хранятся в [`src/i18n/locales/ru.json`](src/i18n/locales/ru.json) и [`src/i18n/locales/en.json`](src/i18n/locales/en.json) с точным совпадением ключей.
- **Осторожно с Tailwind CSS opacity-модификаторами:** в [`src/index.css`](src/index.css) и [`tailwind.config.js`](tailwind.config.js) токены `subtle`, `border-subtle`, `status-*-bg`, `switch-off-*` заданы как CSS-цвета (`rgba`/`hex`), а не RGB-каналы (`--...-rgb`). Никогда не применяйте к ним модификаторы вроде `bg-subtle/50` или `bg-status-danger-bg/80` — это генерирует невалидный CSS. Модификатор `/opacity` поддерживают только токены с `-rgb` (`canvas`, `sidebar`, `surface`, `text-primary`, `text-secondary`, `accent`, `accent-hover`, `accent-fg`, `status-success`, `status-warning`, `status-danger`).

---

## 3. Команды проверки перед коммитом

```powershell
# 1. Фронтенд: типы + сборка Vite
npm run build

# 2. Rust: проверка workspace + тесты ядра
cd src-tauri
cargo check --workspace --offline
cargo test -p isocraft-core --offline
```
