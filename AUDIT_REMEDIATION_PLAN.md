# План исправлений по итогам технического аудита ISOCraft

Документ фиксирует согласованный 7-фазный план устранения всех замечаний (`[Critical]`, `[Major]`, `[Minor]`), выявленных в ходе технического аудита репозитория **ISOCraft** (Tauri v2 / Rust + React 19 + TypeScript + Tailwind CSS v4) по четырём направлениям: архитектура, качество кода и надёжность, UI/UX и анимации.

Все изменения выполняются при строгом соблюдении инвариантов безопасности, UAC, WebView2, ACL/CSP, `ProcessGuard`, оффлайн-реестра и локализации из [`AGENTS.md`](AGENTS.md).

---

## Принятые архитектурные решения и ограничения

- **Контракт ошибок IPC:** структурированный `ApiError { code, params, raw, hint }` на стороне Rust (`isocraft_core::error`) + прямой типобезопасный маппинг по `ErrorCode` на фронтенде (полная замена 652-строчного regex-парсера в [`backendErrors.ts`](src/utils/backendErrors.ts)).
- **Управление состоянием фронтенда:** без добавления новых npm-зависимостей; декомпозиция монолитного [`App.tsx`](src/App.tsx) на доменные хуки/контексты + внешние сторы на `useSyncExternalStore` для высокочастотного и разделяемого состояния (`buildProgress`, `appUpdater`, `logs`) с сохранением состояния посещённых вкладок без размонтирования.
- **Безопасность аргументов Post-Setup ([`post_setup.rs`](src-tauri/crates/core/src/post_setup.rs)):** исключение сырой shell/PowerShell-интерполяции `item.args` (безопасное экранирование/логирование в `before_logon.cmd`, токенизация Winget `extraArgs` и передача массивом PowerShell-литералов в одинарных кавычках `@extraArgs`) с сохранением поддержки легитимных флагов установщиков (пробелы, кавычки, пути вроде `(x86)`).
- **Виртуализация списков:** без новых npm-зависимостей; собственный лёгкий виртуализатор с переменной высотой строк (`useVariableVirtualList` на `ResizeObserver` + prefix-sums + rAF scroll throttling) для `LogDrawer` и крупных экранов.
- **Зависимости и совместимость:**
  - ноль лишних Cargo/npm-зависимостей (Rust-сборка и тесты обязаны проходить с `--locked --offline`);
  - 100% обратная совместимость при чтении существующих пресетов `schemaVersion: "1.0"`;
  - сохранение всех правил из [`AGENTS.md`](AGENTS.md) (UAC `requireAdministrator`, минимальный ACL, строгий CSP, `ProcessGuard` с Job Object, запрет `reg.exe import`, запрет модификации исходного образа на месте, паритет ключей i18n, Tailwind CSS v4 токены).

---

## Фаза 1. Базовая инфраструктура и безопасность `isocraft-core`

### 1.1. Безопасность путей и файловые операции ([`fs_utils.rs`](src-tauri/crates/core/src/fs_utils.rs))
- [x] Устранить UTF-8 панику по срезу `rest[..4]` в `strip_verbatim_for_compare` ([`fs_utils.rs`](src-tauri/crates/core/src/fs_utils.rs#L157)) через безопасный `.get(..4)` и унифицировать её с `strip_windows_verbatim_prefix` ([`env.rs`](src-tauri/crates/core/src/env.rs#L141-L168)).
- [x] Исправить ранний `return false` для `Prefix::Verbatim(..)` в `is_unc_or_device_path` ([`fs_utils.rs`](src-tauri/crates/core/src/fs_utils.rs#L258-L280)): закрыть обход для `\\?\GLOBALROOT\...`, `\\?\Volume{...}`, `\\?\PhysicalDrive*` (пропуская только валидный `Prefix::VerbatimDisk`), оживить недостижимую ветку ([`fs_utils.rs`](src-tauri/crates/core/src/fs_utils.rs#L270-L278)) и обрабатывать смешанные слеши (`/\`, `\/`).
- [x] Разделить `is_path_within_dir` ([`fs_utils.rs`](src-tauri/crates/core/src/fs_utils.rs#L392-L402)) на строгую каноническую проверку вложенности (`is_path_strictly_within_dir` для allow-проверок вроде [`env.rs`](src-tauri/crates/core/src/env.rs#L441) и [`wim.rs`](src-tauri/crates/core/src/wim.rs#L414), пресекающую обход через NTFS-junction; для ещё не созданных на диске путей канонизировать ближайшего существующего предка с проверкой отсутствия reparse-точек и доклеивать несуществующий хвост) и консервативную проверку пересечения (`is_path_overlapping_dir` для deny-проверок вроде `session_current`).
- [x] Перевести `components_equal_ci` ([`fs_utils.rs`](src-tauri/crates/core/src/fs_utils.rs#L228-L242)) с `to_ascii_lowercase()` на Unicode `to_lowercase()` для корректного сравнения кириллических/юникодных путей.
- [x] В `write_file_atomic` ([`fs_utils.rs`](src-tauri/crates/core/src/fs_utils.rs#L320-L377)) добавить атомарный счётчик в имя `.tmp`-файла (исключить коллизии при одинаковом `SystemTime`) и короткий retry-цикл на `MoveFileExW` при `ERROR_SHARING_VIOLATION` / `ERROR_ACCESS_DENIED`.
- [x] Исправить ветку `reject_symlink = false` в `read_file_bounded` ([`fs_utils.rs`](src-tauri/crates/core/src/fs_utils.rs#L56-L73)) — использовать `fs::metadata` вместо `symlink_metadata().is_file()`.
- [x] В `copy_file_cancellable` ([`fs_utils.rs`](src-tauri/crates/core/src/fs_utils.rs#L105-L153)) добавить вариант с переиспользуемым буфером (`copy_file_cancellable_with_buf`, убрать аллокацию 1 MiB на каждый файл при распаковке ISO) и явно отклонять симлинки в `dst`.

### 1.2. Управление процессами и потоковый вывод ([`process.rs`](src-tauri/crates/core/src/process.rs))
- [x] Добавить режим сырого захвата `stdout` без принудительного разрезания строк по `MAX_LINE_BYTES` ([`process.rs`](src-tauri/crates/core/src/process.rs#L13), `stream_lossy_lines` [`#L365-L372`](src-tauri/crates/core/src/process.rs#L365-L372), `push_capped_line` [`#L394`](src-tauri/crates/core/src/process.rs#L394)), чтобы однострочный `ConvertTo-Json -Compress` размером > 64 KiB в [`dism.rs`](src-tauri/crates/core/src/dism.rs#L365) и [`drivers.rs`](src-tauri/crates/core/src/drivers.rs#L227) не повреждался вставкой `\n` и обрывом UTF-8; перевести `read_wim_xml_with_wimlib` ([`image.rs`](src-tauri/crates/core/src/image.rs#L413-L477)) на [`process.rs`](src-tauri/crates/core/src/process.rs).
- [x] Устранить lost-wakeup гонку в `wait_until_idle` ([`process.rs`](src-tauri/crates/core/src/process.rs#L233-L241)) через `tokio::pin!(notified); notified.as_mut().enable();` до проверки `is_busy()`.
- [x] Объединить 5 разрозненных атомиков/мьютексов в `ProcessGuard` ([`process.rs`](src-tauri/crates/core/src/process.rs#L101-L306)) в единый `Mutex<GuardState>` (устранить TOCTOU-гонки в `try_enter_busy_for` и `cancel_operation_kind`), убрать или приватизировать небезопасный `pub fn enter_busy` ([`process.rs`](src-tauri/crates/core/src/process.rs#L159-L166)), унифицировать обработку poisoned mutex и сохранить PID-fallback при ошибке `TerminateJobObject`.
- [x] В `run_tracked_command_with_env` ([`process.rs`](src-tauri/crates/core/src/process.rs#L421-L485)) включить `kill_on_drop(true)`, ограниченный `mpsc::channel` (backpressure), ветку отмены в `tokio::select!` при `rx.recv().await` и RAII-снятие `unregister_pid`.

### 1.3. Выделение `workspace_state.rs` (разрыв цикла `env.rs ↔ wim.rs ↔ registry.rs`)
- [ ] Вынести `WorkspaceState` ([`env.rs`](src-tauri/crates/core/src/env.rs#L43-L47)), `WORKSPACE_STATE_LOCK`, функции чтения/записи/регистрации/снятия и `recover_workspace_async` ([`wim.rs`](src-tauri/crates/core/src/wim.rs#L12-L175), [`#L380-L535`](src-tauri/crates/core/src/wim.rs#L380-L535)) в отдельный модуль `src-tauri/crates/core/src/workspace_state.rs`.
- [ ] Сделать `register_mount_in_state` регистронезависимым; перед снятием регистрации неудачного `mount_wim` ([`wim.rs`](src-tauri/crates/core/src/wim.rs#L283-L298)) проверять реальное состояние через `DISM /Get-MountedImageInfo`.
- [ ] В `recover_workspace_async` при повреждённом `workspace_state.json` (`is_corrupted == true`) объединять список с активными DISM-маунтами из `query_dism_mounted_dirs`, строго лежащими внутри `workspace_dir`; сохранять отсутствующие директории маунтов в `remaining_mounts`, если `DISM /Cleanup-Wim` завершился ошибкой.

### 1.4. Образы, DISM, WIM, ISO, окружение и логирование
- [ ] **Разрыв цикла `image.rs ↔ wim.rs`:** перенести `run_wimlib_command` в [`wim.rs`](src-tauri/crates/core/src/wim.rs).
- [ ] **[`dism.rs`](src-tauri/crates/core/src/dism.rs):**
  - перевести очистку `ro_temp.wim` на RAII-гард (устраняет утечку при отмене/ошибке на [`#L465-L523`](src-tauri/crates/core/src/dism.rs#L465-L523) и [`#L654`](src-tauri/crates/core/src/dism.rs#L654));
  - включить `preferred_lang` в ключ дискового кеша каталога (`v4` на [`#L79`](src-tauri/crates/core/src/dism.rs#L79));
  - не сохранять в кеш пустой список служб при ошибке чтения куста ([`#L608-L636`](src-tauri/crates/core/src/dism.rs#L608-L636));
  - добавить `-ErrorAction Stop` и маркеры обрамления JSON в PowerShell-скрипты;
  - добавить проверку `is_symlink_or_reparse` для `source_path` ([`#L394`](src-tauri/crates/core/src/dism.rs#L394)) и предварительную очистку старого/грязного `ro_mount_dir`;
  - переиспользовать `map_raw_*` в `parse_ps_*_json` ([`#L191-L281`](src-tauri/crates/core/src/dism.rs#L191-L281)), логировать `"Catalog cached"` только при успешной записи и декомпозировать `load_image_catalog` ([`#L379-L659`](src-tauri/crates/core/src/dism.rs#L379-L659), 281 строка).
- [ ] **[`image.rs`](src-tauri/crates/core/src/image.rs):**
  - добавить retry-ожидание появления `DriveLetter` в `mount_iso_drive_letter` ([`#L566-L598`](src-tauri/crates/core/src/image.rs#L566-L598)) и никогда не вызывать `dismount_iso` при ошибке на ISO, который уже был смонтирован пользователем до операции;
  - выделить общий хелпер + RAII-размонтирование ISO для `extract_iso_to_workspace` (утечка маунта при панике/ошибке на [`#L827`](src-tauri/crates/core/src/image.rs#L827)) и `extract_iso_and_export_edition`;
  - отклонять `INDEX=0` и поддержать `Event::CData` в `parse_wim_xml`; улучшить эвристику определения UTF-16LE для не-ASCII текста в `decode_text_bytes`.
- [ ] **[`wim.rs`](src-tauri/crates/core/src/wim.rs):**
  - проверять ошибки `remove_file` и сравнивать `src_wim != target_file` регистронезависимо в `package_install_image_impl` ([`#L643-L654`](src-tauri/crates/core/src/wim.rs#L643-L654), защита от случайного добавления Index 2 в `wimlib`);
  - выровнять постусловие `skip_recompress_wim`; ужесточить `parse_dism_progress`.
- [ ] **[`iso.rs`](src-tauri/crates/core/src/iso.rs):**
  - проверять доступность и доверие `oscdimg` / `xorriso` ДО удаления существующего `output_iso` ([`#L379-L386`](src-tauri/crates/core/src/iso.rs#L379-L386) vs [`#L409-L483`](src-tauri/crates/core/src/iso.rs#L409-L483));
  - логировать причину fallback с недоверенного `oscdimg`; приводить `opts.iso_root` к абсолютному пути;
  - исправить `to_cygwin_path` для длинных verbatim-путей;
  - доработать `validate_autounattend_str` (префиксы пространств имён, текст в хвосте XML, дедупликация веток `Event::Start` и `Event::Empty`);
  - перевести запись `autounattend.xml` и `ei.cfg` на `write_file_atomic`.
- [ ] **[`env.rs`](src-tauri/crates/core/src/env.rs), [`settings.rs`](src-tauri/crates/core/src/settings.rs), [`logger.rs`](src-tauri/crates/core/src/logger.rs):**
  - разделить [`env.rs`](src-tauri/crates/core/src/env.rs) (990 строк) на `env.rs`, `authenticode.rs` и `storage.rs`;
  - искать `oscdimg.exe` через переменные окружения `ProgramFiles` / `ProgramFiles(x86)` вместо хардкода `C:\`;
  - усилить проверку `verify_microsoft_authenticode` (отказ от `#[cfg(all(windows, not(test)))]`, проверка корня Microsoft / EKU) и покрыть тестами;
  - снимать атрибут `ReadOnly` перед удалением в `clear_workspace_storage` и возвращать реально освобождённый объём байт;
  - в `validate_workspace_dir` использовать `GetSystemWindowsDirectoryW`, проверять всю цепочку предков на reparse-точки, блокировать подкаталоги `ProgramFiles`/`ProgramData` и убрать небезопасный fallback на `C:\ProgramData` в `app_data_root()`;
  - сериализовать запись `settings.json`; перевести [`logger.rs`](src-tauri/crates/core/src/logger.rs) на `VecDeque`, ограниченное чтение `read_file_tail_bytes` через `.take()` и проверку на симлинки.

**Проверка фазы 1:**
```powershell
cd src-tauri
cargo fmt --all -- --check
cargo test -p isocraft-core --locked --offline
cargo clippy -p isocraft-core --all-targets --locked -- -D warnings
```

---

## Фаза 2. Доменные модули `isocraft-core`

### 2.1. Post-Setup и интеграция Unattend ([`post_setup.rs`](src-tauri/crates/core/src/post_setup.rs))
- [ ] Устранить command/PowerShell-инъекции через `item.args` ([`post_setup.rs`](src-tauri/crates/core/src/post_setup.rs#L173-L178), [`#L405-L430`](src-tauri/crates/core/src/post_setup.rs#L405-L430), [`#L652-L665`](src-tauri/crates/core/src/post_setup.rs#L652-L665)):
  - в `generate_before_logon_cmd` — убрать raw-интерполяцию `args` в `echo` (логировать только имя payload либо экранировать `^`, `&`, `|`, `<`, `>`, `%`, `!`) и формировать вызов `cmd.exe`/`msiexec`/`exe`/`ps1` без слома кавычек и возможности shell-инъекции;
  - в `generate_first_logon_ps1` для Winget — разбирать `item.args` Win32-совместимым токенизатором (с сохранением кавычек, пробелов и символов вроде `(x86)`) и передавать в `& winget ... @extraArgs` массивом PowerShell-литералов в одинарных кавычках.
- [ ] Исправить `merge_first_logon_into_autounattend` и `ensure_wcm_namespace` ([`post_setup.rs`](src-tauri/crates/core/src/post_setup.rs#L826-L1158)): выбирать `<component name="Microsoft-Windows-Shell-Setup">` с `processorArchitecture="amd64"` (игнорируя `x86`), не ломаться на `/>` внутри значений атрибутов, обновлять `encoding="utf-16"` → `utf-8`, сохранять оригинальное имя и префикс самозакрывающихся тегов при раскрытии, обеспечить идемпотентность слияния (без дублирования `first_logon.cmd`) и `saturating_add(1)` для `<Order>`.
- [ ] Добавить проверку `is_symlink_or_reparse` на целевых файлах + снятие `ReadOnly`/`Hidden`/`System` в `apply_post_setup_to_mount_cancellable` ([`post_setup.rs`](src-tauri/crates/core/src/post_setup.rs#L1322-L1350)).
- [ ] Корректно обрабатывать `exit` в конце пользовательского `SetupComplete.cmd`; перенести обновление `AppInstaller` до 120-секундного ожидания готовности `winget` и проверять `$LASTEXITCODE` после `tar.exe`.

### 2.2. Оффлайн-реестр и `.reg`-парсер ([`registry.rs`](src-tauri/crates/core/src/registry.rs), [`reg_parser.rs`](src-tauri/crates/core/src/reg_parser.rs))
- [ ] Генерировать уникальные имена временных кустов (`ISOCRAFT_{pid}_{nonce}_{suffix}` и `temp_SYSTEM_{nonce}`) и очищать оставшиеся TxR-артефакты `*.blf`/`*.regtrans-ms` (устраняет коллизию `RegLoadKeyW`, удаляющую всё ещё загруженный куст из `workspace_state.json` в [`registry.rs`](src-tauri/crates/core/src/registry.rs#L221-L235)).
- [ ] В `LoadedHiveSession::unload_all` открывать куст с `REG_OPTION_BACKUP_RESTORE` перед `RegFlushKey` и агрегировать ошибки всех выгрузок; в `read_current_control_set_name` логировать предупреждение при fallback на `ControlSet001`.
- [ ] Выделить адаптер чтения/записи служб Windows ([`registry.rs`](src-tauri/crates/core/src/registry.rs#L884-L1293)) в отдельный модуль.
- [ ] Перенести `is_protected_root_delete` ([`registry.rs`](src-tauri/crates/core/src/registry.rs#L38-L53)) в [`reg_parser.rs`](src-tauri/crates/core/src/reg_parser.rs) и валидировать защищённые удаления на этапах `parse_reg_content`, `preview_reg_file` и preflight.
- [ ] В [`reg_parser.rs`](src-tauri/crates/core/src/reg_parser.rs) исправить декодирование `hex(1)` (включая `REGEDIT4`), конвертацию не-ASCII ANSI строк в `REGEDIT4`, остановку на `\0\0` в `REG_MULTI_SZ`, удаление UTF-8 BOM на входе `parse_reg_content` и ограничить переносы `\` только для `hex`-значений.

### 2.3. Службы и MUI-резолвер ([`services.rs`](src-tauri/crates/core/src/services.rs))
- [ ] Добавить проверки `is_symlink_or_reparse` на промежуточных папках и бинарниках в `MuiResolver` ([`services.rs`](src-tauri/crates/core/src/services.rs#L607-L748)), привести ключ кеша `modules` к нижнему регистру и поддержать `\SystemRoot\`.
- [ ] Переставить проверку числового ID ресурса перед `;Fallback` в `parse_indirect_string` ([`services.rs`](src-tauri/crates/core/src/services.rs#L476-L481)).
- [ ] Реализовать разрешение групповых зависимостей вида `+GroupName` в `reclassify_services` ([`services.rs`](src-tauri/crates/core/src/services.rs#L386-L427)).

### 2.4. Сеть, Winget-каталог и Microsoft Update Catalog ([`winget_catalog.rs`](src-tauri/crates/core/src/winget_catalog.rs), [`update_catalog.rs`](src-tauri/crates/core/src/update_catalog.rs))
- [ ] Устранить UTF-8 панику по границе символа в `resolve_redirect_url` ([`update_catalog.rs`](src-tauri/crates/core/src/update_catalog.rs#L523-L527)), ужесточить допустимые символы хоста в `is_allowed_microsoft_host`, исправить кейс-чувствительность `strip_prefix("https://")` и относительные редиректы без ведущего `/`, устранить пропуск проверки хоста при `eff_opt == None` ([`update_catalog.rs`](src-tauri/crates/core/src/update_catalog.rs#L759), [`#L815`](src-tauri/crates/core/src/update_catalog.rs#L815)), починить `extract_attr_value` (пропуск кавычек) и `parse_human_size_bytes`.
- [ ] Убрать `curl -L` до проверки хоста: использовать единый пошаговый резолвер редиректов для `search_microsoft_catalog`, `resolve_catalog_download_urls` и `winget_catalog`, проверять Authenticode на скачанном `source2.msix` и ограничить максимальный размер chunked-ответов.
- [ ] Маршрутизировать все вызовы `curl.exe` и `tar.exe` через `ProcessGuard` (или отдельный отменяемый Job Object для фоновых поисков); синхронизировать `clear_workspace_storage("cache")` с `winget_index_lock`.
- [ ] Перенести временные файлы `search_msstore_packages` ([`winget_catalog.rs`](src-tauri/crates/core/src/winget_catalog.rs#L1264-L1271)) из `%TEMP%` в защищённый подкаталог `<workspace>\cache` с флагом `create_new(true)` и проверкой симлинков.
- [ ] Заменить связку `remove_file` + `rename` на атомарную замену (`MoveFileExW(MOVEFILE_REPLACE_EXISTING)`) для `winget_index.db` и скачанных `.msu`/`.cab`; добавить RAII-очистку `.part`/`.hdr`; кешировать `Content-Length` вместо перечитывания `.hdr` каждые 180 мс; дедуплицировать имена при multi-URL загрузке; проверять промежуточные директории после распаковки `tar -xf`.
- [ ] В `winsqlite::load_api` ([`winget_catalog.rs`](src-tauri/crates/core/src/winget_catalog.rs#L273-L345)) освобождать `HMODULE` при ошибке `get_proc` и не кешировать `Err` навсегда в `OnceLock`.

### 2.5. Обновления, драйверы, приложения, компоненты, файлы и пресеты
- [ ] Выделить общую функцию `run_dism_step`; выполнять `.trim()` для всех путей при исполнении и проверять `is_directory` на preflight; добавить `is_unc_or_device_path` во все прямые IPC-сканеры и на `AddAppxRequest.license_path` ([`pipeline.rs`](src-tauri/src/pipeline.rs#L232-L246)).
- [ ] **[`apps.rs`](src-tauri/crates/core/src/apps.rs):** исправить `classify_appx_arch` (приоритет `x64`/`bundle` над `arm64`, не трактовать `win32` как `x86`), пропускать пустые/пробельные `dependency_paths`, сделать дедупликацию регистронезависимой.
- [ ] **[`updates.rs`](src-tauri/crates/core/src/updates.rs):** убрать wildcard-реэкспорт `pub use crate::update_catalog::*` ([`#L7`](src-tauri/crates/core/src/updates.rs#L7)), сохранять пользовательский порядок очереди обновлений (группируя только смежные `.msu`/`.cab` с ограничением длины командной строки < 30 KiB).
- [ ] **[`drivers.rs`](src-tauri/crates/core/src/drivers.rs):** удалять при ошибке `pnputil` только каталоги, созданные текущим экспортом (не трогать предсуществующие папки на [`#L680`](src-tauri/crates/core/src/drivers.rs#L680)); проверять подкаталоги на симлинки; валидировать имена каталогов при `export_all`; поддерживать расширение `.Inf` и не-DateTime формат поля `Date`.
- [ ] **[`features.rs`](src-tauri/crates/core/src/features.rs):** добавить валидацию имён компонентов/capabilities и явный флаг `limit_access` вместо безусловного `/LimitAccess`.
- [ ] **[`files.rs`](src-tauri/crates/core/src/files.rs):** снимать `HIDDEN`/`SYSTEM`/`READONLY` перед перезаписью; убрать вызов `path.exists()` перед `symlink_metadata` в `scan_custom_file_path`; расширить защиту на `Users\Default\NTUSER.DAT.LOG*`/`.blf`/`.regtrans-ms`, `efi\boot\bootx64.efi`, `boot\bcd`, `efi\microsoft\boot\bcd`; кешировать проверенные каталоги в `apply_file_injections` ([`#L440`](src-tauri/crates/core/src/files.rs#L440), [`#L554`](src-tauri/crates/core/src/files.rs#L554)) — устраняя `O(N * D)` вызовов `canonicalize` и тройной обход дерева папок.
- [ ] **[`preset.rs`](src-tauri/crates/core/src/preset.rs):** добавить `is_unc_or_device_path` при импорте/экспорте, использовать `write_file_atomic`, ввести выделенные DTO `Preset*` для всех секций со 100% обратной совместимостью чтения `schemaVersion: "1.0"`, объединить обход путей в единый хелпер, валидировать `output.target`/`files`/`post_setup`/`drivers` при импорте и заменить `is_file`/`is_dir` на `symlink_metadata` в `path_exists_matching`.

**Проверка фазы 2:**
```powershell
cd src-tauri
cargo fmt --all -- --check
cargo test -p isocraft-core --locked --offline
cargo clippy --workspace --all-targets --locked -- -D warnings
```

---

## Фаза 3. Структурированный `ApiError`, асинхронный IPC и декомпозиция `pipeline.rs`

- [ ] Ввести `isocraft_core::error::{ApiError, ErrorCode}` (`{ code, params, raw, hint }`, `ErrorCode` в `camelCase`, покрывающий все доменные, preflight, XML, `.reg`, DISM-hint и системные ошибки) и перевести все IPC-команды в [`commands.rs`](src-tauri/src/commands.rs) / [`lib.rs`](src-tauri/src/lib.rs) на возврат `Result<T, ApiError>`.
- [ ] Заменить хрупкие проверки `.contains("cancelled")` в [`commands.rs`](src-tauri/src/commands.rs#L429) и [`#L572`](src-tauri/src/commands.rs#L572) на проверку `err.code == ErrorCode::OperationCancelled`.
- [ ] Добавить IPC-команду `log_frontend_error` в [`commands.rs`](src-tauri/src/commands.rs) / [`lib.rs`](src-tauri/src/lib.rs) с ограничением длины сообщения/стека (до 8 KiB) и очисткой управляющих символов для записи критических падений UI из `ErrorBoundary` в `SessionLogger`.
- [ ] Заменить 652-строчный regex-парсер в [`backendErrors.ts`](src/utils/backendErrors.ts) на типизированный маппер `localizeApiError(err)` по `ErrorCode` и `params` (с рекурсивным переводом XML/REG ошибок, без обращения к свойствам `Object.prototype` и без бага с апострофами в путях Windows; с безопасным fallback на `raw`).
- [ ] Перевести синхронные IPC-команды `check_environment`, `save_settings`, `set_app_theme`, `open_external_url`, `reveal_in_explorer` в [`commands.rs`](src-tauri/src/commands.rs#L74) на `async fn` + `tokio::task::spawn_blocking`; устранить TOCTOU-гонку в `save_settings` ([`#L112-L164`](src-tauri/src/commands.rs#L112-L164)); убрать повторный вызов `core_list_host_drivers` внутри `export_host_drivers` ([`#L543`](src-tauri/src/commands.rs#L543)); вынести блокирующий файловый/WIM/DISM/SQLite I/O из `async fn` в `isocraft-core` внутрь `spawn_blocking` / `tokio::fs`.
- [ ] Разделить [`pipeline.rs`](src-tauri/src/pipeline.rs#L309-L1536) (1732 строки в файле, 1228 строк в `run_build_pipeline`) на подмодули `src-tauri/src/pipeline/{preflight,mounts,steps,mod}.rs` с RAII/явным контекстом монтирования (единая точка `/Commit` и гарантированный `/Discard`) и предварительной очисткой остатков `iso_root`/`work` в `session_current` до старта новой сборки.
- [ ] Синхронизировать документацию [`ARCHITECTURE.md`](docs/ARCHITECTURE.md) и [`AGENTS.md`](AGENTS.md) с новой структурой (актуальное число IPC-команд — 36 с учётом `log_frontend_error`, и модули ядра: `workspace_state.rs`, `authenticode.rs`, `storage.rs`, `error.rs`, `pipeline/`).

**Проверка фазы 3:**
```powershell
node scripts/sync-version.mjs --check
npm run check:i18n
npm run build
cd src-tauri
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check --workspace --locked --offline
cargo test --lib --workspace --locked --offline
```

---

## Фаза 4. Декомпозиция состояния фронтенда и устранение гонок

- [ ] Усилить типы в [`studio.ts`](src/types/studio.ts): убрать избыточные расширения `| string` у дискриминируемых union-полей (`ToolStatus.source`, `LogEntry.level`, `UpdateScanInfo.kind`, `InstalledDriverInfo.category`, `RegPreviewOperation.kind`), добавить `ApiErrorPayload`, задействовать строгий `BuildConfig` в `start_build` и унифицировать общий маппер `buildPlanFromQueues(...)` для сборки и пресета с защитой `UNSAFE_KEYS` от prototype pollution.
- [ ] **Внешние сторы на `useSyncExternalStore`:**
  - исправить [`useLogStore.ts`](src/hooks/useLogStore.ts): выполнять `await listen("app-log")` строго до вызова `invoke("get_logs")`, сливать буфер с сортировкой по `id`, удалить неиспользуемые экспорты (`clearLogStore`/`useLogCount`) и сбрасывать состояние в `destroyLogStore`;
  - создать `src/stores/buildProgressStore.ts` для изоляции частых тиков `build-progress` в `BuildProgressCard` и `StatusBar` без перерисовки оболочки;
  - создать `src/stores/appUpdaterStore.ts`, объединив фоновую проверку обновлений при запуске в [`App.tsx`](src/App.tsx) и ручное обновление в [`SettingsModal.tsx`](src/components/SettingsModal.tsx).
- [ ] **Декомпозиция [`App.tsx`](src/App.tsx) и [`usePresetWorkflow.ts`](src/hooks/usePresetWorkflow.ts):**
  - выделить доменные контексты/хуки `StudioSettingsContext`, `ImageSessionContext`, `QueuesContext` (синхронизировать `updatesQueue.runComponentCleanup` с бейджем вкладки «Обновления» и учитывать непустые `output`-настройки в [`ConfirmResetModal.tsx`](src/components/ConfirmResetModal.tsx));
  - разделить [`usePresetWorkflow.ts`](src/hooks/usePresetWorkflow.ts) на чистые модули `presetSerializer.ts`, `presetImporter.ts`, `presetRelinker.ts` и тонкий хук.
- [ ] **Устранение гонок и дефектов состояния:**
  - ввести единый флаг `isGlobalBusy` (`isBuilding`, `inspecting`, `catalogLoading`, `recovering`, `cancellingBuild`, `presetLoading`, `updatingApp`, `clearingStorage`); блокировать смену исходного образа и редакции во время `inspecting` / `catalogLoading`;
  - в `handleStartBuild` проверять `catalogLoading`, после `await save()` читать актуальный снимок состояния из `ref`; в `handleCancelBuild` проверять `isBuilding` и не дублировать `refreshEnvironment()`; добавить обработку ошибок `force_cancel_and_close` в [`App.tsx`](src/App.tsx#L441-L447);
  - в `usePresetWorkflow` захватывать счётчик `seq` ДО `await open()`, выставлять busy-лок (`presetLoading`) до открытия диалогов `open`/`save`, проверять `seq` в `catch`, сохранить `Object.create(null)` + `UNSAFE_KEYS` в `handleRemoveMissingPath` и не проглатывать ошибки сканирования при импорте пресета;
  - убрать нечистый побочный эффект `applyDomTheme` из updater-функции `setSettings`, защитить ранние клики смены языка/темы до завершения `init()` и очищать `themeTransitionTimer` при размонтировании;
  - исправить сброс фазы `beforeLogon` при перелинковке скриптов ([`usePresetWorkflow.ts`](src/hooks/usePresetWorkflow.ts#L961-L964)) и перезапись глобального `customSxsPath` при перелинковке точечного override ([`#L843-L846`](src/hooks/usePresetWorkflow.ts#L843-L846));
  - хранить в состоянии сырые объекты `ApiErrorPayload` вместо предлокализованных строк (динамический перевод баннеров при смене языка RU/EN).
- [ ] Сохранять состояние посещённых экранов без размонтирования со сбросом `scrollTop = 0` при переключении раздела; перенести `<AppContextMenu />` внутрь `<ErrorBoundary>` ([`main.tsx`](src/main.tsx#L11-L14)); в [`ErrorBoundary.tsx`](src/components/ErrorBoundary.tsx) добавить `role="alert"`, сброс `diagStatus` и отправку стека падения через `log_frontend_error`.

**Проверка фазы 4:**
```powershell
npm run test:frontend
npm run check:i18n
npm run build
```

---

## Фаза 5. Производительность UI, кастомный виртуализатор и консистентность UX

- [ ] Реализовать собственный лёгкий виртуализатор с переменной высотой строк `src/hooks/useVariableVirtualList.ts` (`ResizeObserver` + prefix-sums + rAF scroll throttling, без новых npm-зависимостей).
- [ ] Подключить виртуализацию и оптимизировать рендер тяжёлых списков:
  - [`LogDrawer.tsx`](src/components/LogDrawer.tsx): вынести подписку `useLogs()` во внутренний `LogDrawerContent` (0 ререндеров в закрытом состоянии), отслеживать автоскролл по `lastEntryId` вместо `filteredLogs.length`, отключать кнопку «Копировать» при `filteredLogs.length === 0`, добавить защиту от повторного вызова `handleExport` и очистку таймеров;
  - [`ServicesScreen.tsx`](src/components/screens/ServicesScreen.tsx): читать `changeMap` через `ref` в `handleSelectStartType` (восстанавливая `React.memo(ServiceRow)`), рендерить блок деталей только для раскрытых строк, подключить виртуализацию;
  - [`FeaturesScreen.tsx`](src/components/screens/FeaturesScreen.tsx) (фильтровать только активную вкладку + дедупликация JSX списков), [`RegistryScreen.tsx`](src/components/screens/RegistryScreen.tsx), [`AppsScreen.tsx`](src/components/screens/AppsScreen.tsx), [`DriversScreen.tsx`](src/components/screens/DriversScreen.tsx), [`ExportHostDriversModal.tsx`](src/components/screens/ExportHostDriversModal.tsx);
  - [`BuildScreen.tsx`](src/components/screens/BuildScreen.tsx#L205-L379): обернуть вычисление 8 сводок очереди в `useMemo` (устранить пересчёт на каждый тик `build-progress`).
- [ ] Разделить [`SettingsModal.tsx`](src/components/SettingsModal.tsx) (745 строк) на 4 подкомпонента (`WorkspaceSettingsSection`, `ToolsStatusSection`, `AppUpdatesSection`, `StorageSection`), подключить `appUpdaterStore`, добавить проверки `mountedRef`/`clearingTarget`/`refreshingRef`/`isClosing` и `try/catch` в `handleRefresh`.
- [ ] Выделить переиспользуемые UI-примитивы (`FluentButton`, `AlertBanner`, `LoadingStateCard`, `ConfirmDialog`, `SearchInput` с кнопкой очистки), унифицировать фон футеров всех модалок (`bg-sidebar`), дедуплицировать карточки драйверов (`DriversScreen`/`ExportHostDriversModal`), `makeItemId` и `getBaseName`.
- [ ] Устранить неконсистентности UX:
  - пробрасывать `catalogError` и кнопку повторной загрузки в [`AppsScreen.tsx`](src/components/screens/AppsScreen.tsx), [`FeaturesScreen.tsx`](src/components/screens/FeaturesScreen.tsx), [`ServicesScreen.tsx`](src/components/screens/ServicesScreen.tsx) и [`DriversScreen.tsx`](src/components/screens/DriversScreen.tsx);
  - прокинуть `isBuilding`/`isBusy` в `EmptyStateCard`-кнопки, в `FluentSegmentedControl` (`FilesScreen`, `PostSetupScreen`), пробросить `recovering`/`cancellingBuild` в `BuildScreen` ([`App.tsx`](src/App.tsx#L1163)) и `presetLoading` во все экраны;
  - корректно отображать `0.0 GB` и `lowSpaceWarning` при нулевом свободном месте (не скрывать проверкой `> 0` в `StatusBar` и `SettingsModal`);
  - добавить `overflow-y-auto` в [`Sidebar.tsx`](src/components/Sidebar.tsx) с учётом `scrollTop` внутри `measureItem`;
  - добавить подсказку минимальной длины запроса при вводе 1 символа в [`AddWingetModal.tsx`](src/components/screens/AddWingetModal.tsx);
  - обернуть широкие `FluentSegmentedControl` ([`DriversScreen.tsx`](src/components/screens/DriversScreen.tsx), [`PostSetupPreviewModal.tsx`](src/components/screens/PostSetupPreviewModal.tsx)) для горизонтальной прокрутки или переноса;
  - убрать дублирующуюся иконку `RotateCcw` рядом с `Keep` в [`AppsScreen.tsx`](src/components/screens/AppsScreen.tsx);
  - добавить индикаторы загрузки и накопление ошибок при пакетном добавлении файлов, уточнить текст и доступность основной кнопки в [`StatusBar.tsx`](src/components/StatusBar.tsx), локализовать `scanError` в [`AddAppxModal.tsx`](src/components/screens/AddAppxModal.tsx#L249) и заменить нативный `<select>` на Fluent dropdown;
  - добавить очистку таймера копирования в [`PostSetupPreviewModal.tsx`](src/components/screens/PostSetupPreviewModal.tsx) и заменить некорректный `aria-label={t("logDrawer.close")}` на баннерах в [`BuildScreen.tsx`](src/components/screens/BuildScreen.tsx#L468) и [`#L582`](src/components/screens/BuildScreen.tsx#L582).
- [ ] Закрыть edge cases в утилитах [`format.ts`](src/utils/format.ts), [`win11Version.ts`](src/utils/win11Version.ts), [`driverCategories.ts`](src/utils/driverCategories.ts), [`destinationValidation.ts`](src/utils/destinationValidation.ts), [`contextMenu.ts`](src/utils/contextMenu.ts).

**Проверка фазы 5:**
```powershell
npm run test:frontend
npm run check:i18n
npm run build
```

---

## Фаза 6. Анимации, плавность и доступность (WCAG / ARIA)

### 6.1. Контрастность и доступность (WCAG и ARIA)
- [ ] Зарегистрировать `--color-status-danger-fg: rgb(var(--status-danger-fg-rgb));` в `@theme inline` и добавить токен `--status-danger-fg-rgb` в `:root` / `html.dark` в [`index.css`](src/index.css#L5-L69) (аналогично `--accent-fg-rgb`, обеспечивая контраст ≥ 4.5:1 вместо ~2.08:1 в тёмной теме) и заменить `bg-status-danger text-white` на всех 7 danger-кнопках.
- [ ] Сделать проп `ariaLabel` обязательным на всех `FluentModal`; перенести `Tab` focus-trap на уровень `document`; передать `{ locked: stopping }` в [`CloseGuardModal.tsx`](src/components/CloseGuardModal.tsx#L17-L18).
- [ ] Расставить недостающие ARIA-атрибуты и клавиатурную навигацию: `aria-current="page"` ([`Sidebar.tsx`](src/components/Sidebar.tsx)), `aria-expanded` / `aria-controls` / `aria-live="polite"` ([`StatusBar.tsx`](src/components/StatusBar.tsx)), `role="log"` и `aria-label` ([`LogDrawer.tsx`](src/components/LogDrawer.tsx)), `aria-label` на иконках закрытия/удаления и полях поиска, `htmlFor`/`id` для всех `<label>`, `role="radiogroup"`/`role="radio"` ([`ImageScreen.tsx`](src/components/screens/ImageScreen.tsx)), `aria-pressed` на чипах фильтров, `ariaLabel` на `FluentSegmentedControl`, отсутствие скрытых фокусируемых элементов в свёрнутых строках [`ServicesScreen.tsx`](src/components/screens/ServicesScreen.tsx), клавиатурное управление строками в [`RegistryScreen.tsx`](src/components/screens/RegistryScreen.tsx), `listbox`-навигацию в [`CatalogModal.tsx`](src/components/screens/CatalogModal.tsx), `aria-activedescendant` и позиционирование по `Shift+F10` в [`AppContextMenu.tsx`](src/components/AppContextMenu.tsx), единые `focus-visible`-кольца на всех интерактивных контролах.

### 6.2. Модальные окна и FLIP-переупорядочивание
- [ ] В [`FluentModal.tsx`](src/components/FluentModal.tsx) добавить `pointer-events-none` на время `isClosing`, стабилизировать `modalStack` через `ref` (без перерегистрации на ререндерах) и выровнять анимации закрытия (`ConfirmResetModal`, `ExportHostDriversModal`).
- [ ] Вынести единый хук `src/hooks/useFlipReorder.ts`: строгое разделение фаз READ → WRITE без forced reflow, привязка `ref` ко внешнему `<div>` в [`RegistryScreen.tsx`](src/components/screens/RegistryScreen.tsx#L290-L295) и страховочный таймер очистки `transition`/`transform`.
- [ ] В [`index.css`](src/index.css#L116-L187) завершать keyframes модалок, меню и элементов (`fluent-modal-enter`, `fluent-menu-enter`, `fluent-item-enter`) значением `transform: none` во избежание застревания stacking context и конфликта с FLIP.

### 6.3. Устранение layout-триггерящих анимаций и дёрганий
- [ ] Заменить анимацию геометрических свойств на GPU-композитные (`transform` / `opacity`):
  - `height` в [`LogDrawer.tsx`](src/components/LogDrawer.tsx);
  - `width` и постоянный `will-change` в `FluentSegmentedControl` ([`FluentMotion.tsx`](src/components/FluentMotion.tsx#L87));
  - `height` и постоянный `will-change` на индикаторах активной/наведённой вкладки в [`Sidebar.tsx`](src/components/Sidebar.tsx);
  - `width` → `scaleX` с `origin-left` во всех 4 прогресс-барах.
- [ ] Удалить глобальный селектор `html.theme-transitioning *` с `!important` (ограничить переход крупными контейнерами оболочки); заменить 138 вхождений `transition-all` на целевые свойства (`transition-colors`, `transition-[transform,opacity]`).
- [ ] В [`ServicesScreen.tsx`](src/components/screens/ServicesScreen.tsx) убрать скачок `mt-2.5` при раскрытии строки и заменить тяжёлую анимацию `grid-template-rows` на лёгкое композитное появление условно смонтированного блока.
- [ ] Исправить в [`AppContextMenu.tsx`](src/components/AppContextMenu.tsx) измерение размеров до фиксации координат (без `.animate-fluent-menu`), добавить `key` на открытие (чтобы повторный ПКМ проигрывал анимацию заново), подключить `.animate-fluent-menu-exit`, починить проверку `isConnected` в `cut` и двойную мутацию в `insertTextAtSelection`.
- [ ] Убрать принудительное перемонтирование SVG `<g key={animKey}>` в [`SidebarIcons.tsx`](src/components/SidebarIcons.tsx), унифицировать иконку `features`, заменить некомпозитный `stroke-dashoffset` в `.animate-sidebar-check-draw` ([`index.css`](src/index.css#L534-L537)), убрать построчный `<FluentReveal>` в [`DriversScreen.tsx`](src/components/screens/DriversScreen.tsx#L626) и вложенные `<FluentReveal>` в [`BuildScreen.tsx`](src/components/screens/BuildScreen.tsx).

### 6.4. Motion-токены и `prefers-reduced-motion`
- [ ] Ввести единые CSS-переменные длительностей и кривых Безье (Fluent motion tokens) в [`index.css`](src/index.css) и синхронизировать JS-таймеры с CSS (`FluentModal` 150 vs 140 мс, `LogDrawer` 160 vs 150 мс, `CatalogModal` 110 vs 100 мс), скорости спиннеров (`animate-spin` 1000 мс vs `.animate-fluent-spin-loop` 650 мс) и длительности прогресс-баров (150/200/300 мс).
- [ ] В `@media (prefers-reduced-motion: reduce)` ([`index.css`](src/index.css#L566-L602)) подавить `.animate-spin` (17 использований), добавить `transition-delay: 0ms !important`, отключить геометрические hover/active-трансформации (`rotate-45`, `scale-x`, ползунки `features` в [`SidebarIcons.tsx`](src/components/SidebarIcons.tsx), шестерёнку в [`Titlebar.tsx`](src/components/Titlebar.tsx), ползунок в [`FluentSwitch.tsx`](src/components/FluentSwitch.tsx)) и пропускать тики анимаций иконок при сниженном движении.
- [ ] Отвязать сброс состояния спиннера (`refreshSpinning`/`updateCheckSpinning`) в [`SettingsModal.tsx`](src/components/SettingsModal.tsx) от события `onAnimationIteration` (чтобы не зависало при `reduced-motion`), унифицировать `active:scale-*` с обязательным `disabled:active:scale-100`.

**Проверка фазы 6:**
```powershell
npm run check:i18n
npm run test:frontend
npm run build
```

---

## Фаза 7. Регрессионные тесты и финальная верификация

- [ ] Написать юнит-тесты в `isocraft-core` и `isocraft` для всех исправленных сценариев фаз 1–3: 64 KiB+ однострочный UTF-8 JSON через `process.rs`, lost-wakeup стресс-тест `ProcessGuard::wait_until_idle`, конкурентный `cancel_operation_kind`, не-ASCII `\\?\`-пути и junction-обход в `is_path_strictly_within_dir`, параллельные `write_file_atomic`, восстановление живых маунтов при повреждённом `workspace_state.json`, безопасная токенизация/экранирование аргументов `post_setup.rs`, `merge_first_logon_into_autounattend` (dual-arch `x86`+`amd64`, UTF-16 XML и идемпотентность), `is_protected_root_delete` на preview, `REGEDIT4` `hex(1)` и `\0\0`, multi-arch `classify_appx_arch`, `+GroupName` и `;Fallback` в `services.rs`, не-ASCII `Location:` в `resolve_redirect_url`, паритет пресетов `schemaVersion: "1.0"`, контракты `ApiError` и ограничение длины в `log_frontend_error`.
- [ ] Расширить [`test-frontend.mjs`](scripts/test-frontend.mjs) тестами на все исправленные модули фаз 3–6: маппинг кодов `localizeApiError`, порядок и дедупликацию `useLogStore`, `buildProgressStore`, `appUpdaterStore`, сериализатор/импорт/релинк пресетов и защиту от `UNSAFE_KEYS`, расчёт диапазонов `useVariableVirtualList` для строк переменной высоты, утилиты (`formatBytes`, `resolveWin11Version`, `driverCategories`, `destinationValidation`, `contextMenu`) и проверку полноты `prefers-reduced-motion` в [`index.css`](src/index.css).
- [ ] Пройти полный чеклист проверки из [`AGENTS.md §3`](AGENTS.md#L73-L88):

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
