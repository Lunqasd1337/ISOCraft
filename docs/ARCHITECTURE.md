# Архитектура ISOCraft

Документ описывает внутреннее устройство приложения **ISOCraft**, 9-шаговый пайплайн сборки, IPC-интерфейс Tauri и процедуры верификации.

---

## 1. Системная схема и модули

### Backend (`src-tauri/`)
- **`src-tauri/src/`** (оболочка Tauri v2, `isocraft`):
  - `lib.rs` — точка входа (`run`), настройка WebView2 (`webview_user_data_dir()`), перехват `WindowEvent::CloseRequested` и регистрация IPC-команд (`commands::*`).
  - `state.rs` — `AppState`, структуры `BuildConfig` / `BuildProgressEvent` / `AppsBuildPlan` / `FeaturesBuildPlan`, потоковые эмиттеры `log_and_emit` / `emit_progress`, `apply_window_theme` и `cleanup_session_dir_if_clean`.
  - `commands.rs` — 24 `#[tauri::command]` IPC-обработчика (`check_environment`, `inspect_image`, `load_image_catalog`, команды сканирования, пресетов, `start_build`, `cancel_build`, `recover_workspace`, `force_cancel_and_close`).
  - `pipeline.rs` — оркестратор 9-шагового пайплайна `run_build_pipeline` и проверка защиты от перезаписи исходного образа (`paths_overlap_source`).
- **`src-tauri/crates/core/src/`** (`isocraft-core`, чистое ядро без UI/диалогов):
  - `env.rs` — определение прав администратора, поиск утилит (`dism.exe`, `curl.exe`, `powershell.exe`, `explorer.exe`, `wimlib-imagex.exe`, `xorriso.exe`, `oscdimg.exe`), снятие Windows verbatim-префикса (`strip_windows_verbatim_prefix` для `\\?\` / `\\?\UNC\`) и нормализация разделителей для `resource_dir()`, проверка свободного места (`< 40 ГБ`), чтение `workspace_state.json`.
  - `settings.rs` — хранение настроек в `%LOCALAPPDATA%\ISOCraft\settings.json`, рабочая папка по умолчанию `%LOCALAPPDATA%\ISOCraft\work`, WebView2 в `%LOCALAPPDATA%\ISOCraft\WebView2`.
  - `logger.rs` — кольцевой буфер логов сессии (`LogEntry`) и экспорт диагностики (`dism.log` + лог приложения).
  - `process.rs` — `ProcessGuard`, запуск CLI без окна консоли (`CREATE_NO_WINDOW`) с построчным стримингом stdout/stderr и принудительное завершение дерева процессов через Win32 ToolHelp (`kill_process_tree`).
  - `image.rs` — разбор XML `wimlib-imagex info --xml` (`quick-xml`), фильтрация клиентской Windows 11 x64, `Mount-DiskImage` распаковка ISO и рекурсивное снятие атрибута `ReadOnly`.
  - `wim.rs` — `export_single_edition`, `mount_wim`/`unmount_wim` с фиксацией в `workspace_state.json`, `recover_workspace_async`, `package_install_image` (`WIM` / `ESD --compress=LZMS --solid` / `SWM`).
  - `iso.rs` — строгая XML-валидация `autounattend.xml`, генерация `sources\ei.cfg`, сборка загрузочных ISO через `oscdimg.exe` или `xorriso.exe -as mkisofs`.
  - `dism.rs` — загрузка и дисковое кеширование каталога редакции (`catalog_*.json`) через однократный RO-маунт + PowerShell `Dism` JSON + немедленный `/Discard`.
  - `apps.rs` — извлечение `PackageFamilyName`, словарь защищённых системных пакетов, фильтрация зависимостей по архитектуре (`x64`/`neutral`), вызовы DISM `/Add-ProvisionedAppxPackage` и `/Remove-ProvisionedAppxPackage`.
  - `features.rs` — включение/отключение Optional Features (`/All`, `/Source sxs`, `/LimitAccess`) и Capabilities.
  - `updates.rs` — сканирование `.msu`/`.cab` и папок, пакетная интеграция 24H2+ Checkpoint CU, расшифровка ошибок DISM, `StartComponentCleanup` (с безопасным пропуском `0x800f0806` в оффлайне) и реэкспорт `update_catalog`.
  - `update_catalog.rs` — поиск (`Search.aspx`), разбор диалогов (`DownloadDialog.aspx`) и HTTPS-only загрузка обновлений из Microsoft Update Catalog в `<workspace>\downloads\updates`.
  - `drivers.rs` — сканирование `.inf` драйверов, передача `/Recurse` (только для директорий) и `/ForceUnsigned`, поддержка `install.wim` и `boot.wim` Index 2.
  - `reg_parser.rs` — парсер `.reg` v5 / `REGEDIT4` (UTF-16LE/BE/UTF-8, `\`, `@`, все типы значений), маппинг веток (`CurrentControlSet`, `HKCU`, `HKCR`, `HKU\.DEFAULT`) и dry-run предпросмотр.
  - `registry.rs` — нативная Win32 загрузка/выгрузка оффлайн-кустов и применение операций с правами `TrustedInstaller` (`REG_OPTION_BACKUP_RESTORE` + `NtDeleteKey`).
  - `files.rs` — проверка и копирование пользовательских файлов и папок в `mount_install` и `iso_root` с политиками конфликта `Overwrite` / `Skip` и защитой системных путей.
  - `preset.rs` — сериализация/импорт JSON-пресетов (`schemaVersion: "1.0"`) и аудит отсутствующих локальных путей.

### Frontend (`src/`)
- **`src/App.tsx`** — основное состояние очередей, каталога, тем и сборки.
- **`src/hooks/usePresetWorkflow.ts`** — сохранение/импорт пресетов (`schemaVersion: "1.0"`) и relink/remove отсутствующих локальных путей.
- **`src/utils/format.ts`** — общие утилиты форматирования (`formatBytes`).
- **`src/components/`** — `Titlebar.tsx` (40px, переключение языка и темы), `Sidebar.tsx` (220px, 8 разделов с бейджами очереди), `StatusBar.tsx` (48px), `LogDrawer.tsx` (200px), `FluentModal.tsx` (общая обёртка модальных окон + `useAnimatedModalClose`), `SettingsModal.tsx`, `CloseGuardModal.tsx`.
- **`src/components/screens/`** — `ImageScreen.tsx`, `AppsScreen.tsx` (+ `AddAppxModal.tsx`), `FeaturesScreen.tsx`, `UpdatesScreen.tsx` (+ `CatalogModal.tsx`), `DriversScreen.tsx`, `RegistryScreen.tsx`, `FilesScreen.tsx`, `BuildScreen.tsx`, `ScreenShell.tsx`.

---

## 2. 9-шаговый пайплайн сборки (`start_build`)

Перед стартом проверяется доступность `dism.exe`, `wimlib-imagex.exe`, инструмента ISO (`oscdimg.exe` / `xorriso.exe`, если `target == "iso"`), а также валидность всех правил раздела «Файлы».

1. **Подготовка рабочей директории:** при источнике `.iso` выполняется `Mount-DiskImage` → копирование во временную папку `<workspace>\session_current\iso_root` → рекурсивное снятие `ReadOnly` → `Dismount-DiskImage`.
2. **Одноредакционный экспорт:** `wimlib-imagex export` выбранного индекса в `<workspace>\session_current\work\install.wim` (становится Index `1`, остальные редакции вырезаются).
3. **Монтирование (RW):** если есть хотя бы одно изменение для `install.wim` (компоненты, обновления, Appx, драйверы для `install.wim`, `.reg`, файлы для `install.wim`, очистка), монтируется `install.wim` в `mount_install`. Если `target == "iso"` и есть драйверы с флагом `boot.wim`, дополнительно монтируется `iso_root\sources\boot.wim` (**Index `2`**) в `mount_boot`. Все активные маунты регистрируются в `workspace_state.json`.
4. **Компоненты Windows:** Optional Features (с подстановкой `sxs` из `iso_root\sources\sxs` или ручного пути, `/All`) и Capabilities (`/LimitAccess`). Выполняется до обновлений для согласованности пакетов.
5. **Обновления (`.msu` / `.cab` / папка 24H2+):** вызов `DISM /Add-Package` для папок и батча файлов с разрешением цепочки Checkpoint CU → Target CU.
6. **Приложения (Appx / MSIX):** разрешение `PackageFamilyName` → актуальный `PackageName` в смонтированной редакции, удаление отмеченных пакетов, добавление новых AppX/MSIX с зависимостями (`x64`/`neutral`), лицензией, `StubPackageOption` и `/Region:all`.
7. **Драйверы (`.inf`):** инъекция в `mount_install` и при необходимости в `mount_boot` (Setup, Index 2).
8. **Очистка, оффлайн-реестр и пользовательские файлы:**
   - Опциональный `DISM /Cleanup-Image /StartComponentCleanup` (при коде `0x800f0806` / pending operations шаг безопасно пропускается с предупреждением).
   - Нативное применение включённых `.reg`-файлов в `mount_install` (с гарантированной выгрузкой всех кустов и `RegUnLoadKeyW` до завершения шага).
   - Копирование пользовательских файлов и папок в `mount_install` (`FileTarget::InstallWim`).
   - При любой ошибке шагов 4–8 выполняется автоматический откат (`/Discard` обоих маунтов).
9. **Финализация и упаковка:**
   - `Unmount-Image /Commit` для `mount_boot` и `mount_install` (при сбое коммита `boot.wim` выполняется `/Discard` для `mount_install`).
   - Конвертация/упаковка `work\install.wim` в итоговый `WIM`, `ESD` (`LZMS --solid`) или `SWM` (нарезка `wimlib-imagex split`).
   - Если `target == "iso"`: копирование пользовательских файлов для `iso_root` (`FileTarget::IsoRoot`), встраивание проверенного `autounattend.xml` и опционального `sources\ei.cfg`, сборка загрузочного ISO (`oscdimg.exe` или `xorriso.exe`).

---

## 3. Контракт IPC (Tauri Commands & Events)

### Команды (`invoke`)

| Команда | Назначение |
| :--- | :--- |
| `check_environment` | Проверка прав администратора, статуса `dism`/`wimlib`/`xorriso`/`oscdimg`, свободного места на диске и наличия `workspace_state.json`. |
| `get_settings` / `save_settings` | Чтение и сохранение `AppSettings` (`workspaceDir`, `oscdimgPath`, `theme`, `language`). |
| `set_app_theme` | Переключение темы окна Tauri (`light` \| `dark` \| `system`) и сохранение настройки. |
| `get_logs` / `export_diagnostics` | Получение снимка логов сессии; экспорт отчёта (лог приложения + хвост `dism.log`). |
| `inspect_image` | Инспекция `.iso`/`.wim`/`.esd`, проверка клиентской Windows 11 x64 и списка редакций. |
| `load_image_catalog` | Получение/кеширование списков Provisioned Appx, Optional Features и Capabilities для индекса. |
| `scan_appx_dependencies` | Фильтрация зависимостей AppX/MSIX по архитектуре (`x64`/`neutral` допускаются, `x86`/`arm64` отсеиваются). |
| `scan_update_path` | Подсчёт `.msu`/`.cab` пакетов и размера файла/папки обновлений. |
| `search_update_catalog` | Поиск обновлений в Microsoft Update Catalog (`Search.aspx`) с фильтрацией Windows 11 x64. |
| `download_catalog_update` | Разрешение прямых ссылок (`DownloadDialog.aspx`) и загрузка `.msu`/`.cab` в `<workspace>\downloads\updates`. |
| `cancel_catalog_download` | Прерывание активной загрузки обновления из каталога Microsoft (`ProcessGuard`). |
| `scan_driver_folder` | Подсчёт `.inf` файлов и инспекция папки/файла драйвера. |
| `scan_custom_file_path` | Инспекция пользовательского файла или папки для очереди раздела «Файлы». |
| `preview_reg_file` | Dry-run парсинг `.reg` (затрагиваемые кусты, статистика операций, предупреждения, ошибки). |
| `validate_autounattend` | Строгая XML-проверка `autounattend.xml` (корневой элемент `unattend`, стек тегов и `settings[@pass]`). |
| `export_preset` / `import_preset` | Сохранение и загрузка JSON-пресета (`schemaVersion: "1.0"`) с отчётом об отсутствующих локальных путях. |
| `open_external_url` | Открытие в браузере разрешённых ссылок (`ALLOWED_URLS`: Microsoft Win11 Download, UUP dump, Microsoft Update Catalog, GitHub и Releases). |
| `start_build` / `cancel_build` | Запуск 9-шагового пайплайна; безопасная отмена (убийство дерева процессов + откат). |
| `recover_workspace` | Принудительная выгрузка зависших кустов реестра (`RegUnLoadKeyW`), `/Discard` маунтов и `DISM /Cleanup-Wim`. |
| `force_cancel_and_close` | Завершение активных дочерних процессов, очистка рабочей директории и выход по подтверждению `CloseGuardModal`. |

### События (`listen`)
- `app-log` (`LogEntry`) — потоковая запись логов бэкенда и CLI в журнал UI.
- `build-progress` (`BuildProgressEvent`) — текущий шаг (`1..9`), `phaseKey`, процент и сообщение статуса.
- `catalog-download-progress` (`CatalogDownloadProgress`) — прогресс загрузки `.msu`/`.cab` файла из Microsoft Update Catalog (`fileIndex`, `totalFiles`, `downloadedBytes`, `totalBytes`, `percent`).
- `close-requested-while-busy` — запрос подтверждения у пользователя при попытке закрыть окно во время активной операции.

---

## 4. Crash Recovery и рабочие каталоги

- Данные приложения хранятся в `%LOCALAPPDATA%\ISOCraft\`:
  - `settings.json` — пользовательские настройки;
  - `WebView2\` — профили WebView2 (изолировано под правами UAC);
  - `work\` — рабочая директория (можно перенести на другой диск через Настройки):
    - `cache\catalog_*.json` — кеш каталогов редакций;
    - `downloads\updates\` — загруженные из Microsoft Update Catalog пакеты `.msu`/`.cab`;
    - `dism.log` — выделенный лог DISM текущей рабочей директории;
    - `workspace_state.json` — журнал активных точек монтирования и временных имён кустов реестра (`ISOCRAFT_<pid>_<HIVE>`); удаляется только после полного размонтирования.
- При аварийном завершении на следующем запуске `check_environment` обнаруживает `workspace_state.json` и отображает баннер восстановления (**«Очистить сейчас»**).

---

## 5. Чек-лист ручной проверки в VM

Для полной интеграционной приёмки на реальном образе Windows 11 x64 (23H2 / 24H2+):
1. Проверить одноредакционный экспорт на многоредакционном ISO (в итоговом образе должен остаться строго 1 индекс).
2. Проверить удаление и добавление Appx/MSIX с зависимостями, включение `NetFx3` из `sources\sxs` / ручной папки `sxs`, интеграцию папки обновлений 24H2+ (*Checkpoint CU + LCU*).
3. Проверить инъекцию драйверов в `install.wim` и `boot.wim` Index 2, применение `.reg` (включая `NTUSER.DAT`, `CurrentControlSet` и удаление `TrustedInstaller`-ключа `[-Key]`), а также копирование пользовательских файлов в `C:\` и корень ISO.
4. Проверить сохранение/загрузку JSON-пресета со сценарием перемещённого файла (подсветка + «Указать путь...»).
5. Проверить загрузку и установку собранного ISO в VM (Hyper-V / VMware / QEMU) в режимах **UEFI** (`efisys_noprompt.bin`) и **Legacy BIOS** (`etfsboot.com`), а также с `install.esd`, `install.swm*` и `autounattend.xml`.
