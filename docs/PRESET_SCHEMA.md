# Схема JSON-пресетов ISOCraft (версия `1.0`)

Пресет сохраняет конфигурацию сборки (намерения изменений и параметры вывода) в переносимом JSON-формате без привязки к исходному `.iso`/`.wim`-файлу и временным путям рабочей директории. За импорт, экспорт и аудит локальных путей отвечает [`preset.rs`](../src-tauri/crates/core/src/preset.rs) и фронтенд-хук [`usePresetWorkflow.ts`](../src/hooks/usePresetWorkflow.ts).

---

## 1. Общие правила формата

- **Версия схемы:** поддерживается строго `schemaVersion: "1.0"`. Несовместимая версия отклоняется с ошибкой при импорте.
- **Кодировка и размер:** UTF-8; при чтении автоматически удаляется UTF-8 BOM (`\u{feff}`). Максимальный размер файла пресета — **16 МиБ**. При импорте сам файл пресета не может быть символической ссылкой или NTFS reparse-точкой (`read_file_bounded` с `reject_symlink: true`).
- **Безопасность путей:** любые сетевые UNC-пути и пути устройств Windows (`\\server\share\...`, `\\.\...`) запрещены (`reject_unc_paths_in_preset`) и отклоняют импорт. При экспорте приложение отказывается перезаписывать символическую ссылку или reparse-точку.
- **Совместимость:**
  - Неизвестные/устаревшие поля (например, `output.splitSizeMb`) безопасно игнорируются при десериализации.
  - Удалённый формат `output.installFormat: "swm"` отклоняется с ошибкой разбора.
  - При экспорте и импорте `output.target` приводится к нижнему регистру (при пустом значении подставляется `"iso"`). В UI любое значение, отличное от `"iso"`, либо импорт ISO-пресета поверх открытого `.wim` или `.esd` приводит `output.target` к `"image"` и сбрасывает флаг `injectToBootWim` у драйверов.

---

## 2. Структура верхнего уровня (`StudioPreset`)

```json
{
  "schemaVersion": "1.0",
  "meta": {},
  "apps": {},
  "features": {},
  "services": {},
  "updates": {},
  "drivers": {},
  "registry": {},
  "files": {},
  "postSetup": {},
  "output": {}
}
```

Все разделы кроме `schemaVersion` имеют значения по умолчанию (`#[serde(default)]`) и могут быть опущены в минимальных вручную написанных пресетах.

---

## 3. Спецификация разделов

### 3.1. `meta` (`PresetMeta`)

| Поле | Тип | По умолчанию | Описание |
| :--- | :--- | :--- | :--- |
| `name` | `string` | `""` | Название пресета. |
| `description` | `string` | `""` | Произвольное описание профиля. |
| `createdAt` | `string` | UTC RFC-3339 | Метка времени создания (заполняется автоматически при экспорте, если пустая). |
| `targetArch` | `string` | `"x64"` | Целевая архитектура. |
| `targetEditionHint` | `string \| null` | `null` | Информационная подсказка редакции (например, `"Windows 11 Pro"`). |

### 3.2. `apps` (`PresetApps`)

- **`removePackages`**: массив удаляемых Provisioned Appx. Поддерживаются два формата:
  - Объект (рекомендуется, создаётся при экспорте): `{ "packageFamilyName": string, "displayNameHint"?: string | null }`
  - Строка (обратная совместимость): `"Microsoft.BingNews_8wekyb3d8bbwe"`
  Во время сборки `packageFamilyName` автоматически разрешается в точный `PackageName` смонтированной редакции.
- **`addPackages`**: массив объектов `PresetAddAppx` (соответствует `AddAppxRequest` во фронтенде):

| Поле | Тип | По умолчанию | Описание |
| :--- | :--- | :--- | :--- |
| `packagePath` | `string` | `""` | Путь к `.appx`, `.appxbundle`, `.msix` или `.msixbundle`. |
| `dependencyPaths` | `string[]` | `[]` | Список локальных путей к пакетам зависимостей (`x64`/`neutral`). |
| `licensePath` | `string \| null` | `null` | Путь к XML-лицензии (если `skipLicense: false`). |
| `skipLicense` | `boolean` | `true` | Передавать ли DISM флаг `/SkipLicense`. |
| `regionAll` | `boolean` | `true` | Передавать ли DISM флаг `/Region:all`. |
| `stubOption` | `"None" \| "UseStub" \| "UsePreference"` | `"None"` | Режим `/StubPackageOption`. |

### 3.3. `features` (`PresetFeatures`)

| Поле | Тип | По умолчанию | Описание |
| :--- | :--- | :--- | :--- |
| `enable` | `PresetFeatureEnable[]` | `[]` | Включаемые компоненты: `{ "name": string, "enableAll": boolean (true), "customSxsPath": string \| null }`. |
| `disable` | `string[]` | `[]` | Системные имена отключаемых Optional Features. |
| `capabilitiesAdd` | `PresetCapabilityAdd[]` | `[]` | Добавляемые Capabilities: `{ "name": string, "limitAccess": boolean (true), "customSxsPath": string \| null }`. |
| `capabilitiesRemove` | `string[]` | `[]` | Имена удаляемых Capabilities. |
| `defaultSxsPath` | `string \| null` | `null` | Пользовательская папка `sxs` по умолчанию (перекрывает автоподстановку `sources\sxs` из ISO). |

> [!NOTE]
> При загрузке пресета в UI (`usePresetWorkflow.ts`) общий путь `customSxsPath` берётся из `defaultSxsPath` (или первого заданного `customSxsPath` среди `enable`/`capabilitiesAdd`), а индивидуальные настройки `enableAll`, `limitAccess` и `customSxsPath` сохраняются в `featureOverrides`/`capabilityOverrides` и воспроизводятся при следующем экспорте и сборке.

### 3.4. `services` (`ServicesBuildPlan`, опциональный раздел)

| Поле | Тип | По умолчанию | Описание |
| :--- | :--- | :--- | :--- |
| `changes` | `{ name: string, startType: "automatic" \| "delayedAuto" \| "manual" \| "disabled" }[]` | `[]` | Список изменений типа запуска Win32-служб в `SYSTEM\ControlSetNNN\Services`. Дубликаты имён и отключение заблокированных критических служб отклоняются при импорте и перед сборкой; службы, отсутствующие в целевой редакции, пропускаются с предупреждением. |

### 3.5. `updates` (`UpdatesBuildPlan`)

| Поле | Тип | По умолчанию | Описание |
| :--- | :--- | :--- | :--- |
| `items` | `{ path: string, isDirectory: boolean }[]` | `[]` | Очередь `.msu` / `.cab` файлов или папок с обновлениями (в т.ч. папок 24H2+ с Checkpoint CU). Внутри каждого элемента оба поля (`path`, `isDirectory`) обязательны. |
| `runComponentCleanup` | `boolean` | `false` | Выполнять ли `DISM /Cleanup-Image /StartComponentCleanup`. |
| `cleanupResetBase` | `boolean` | `false` | Передавать ли `/ResetBase` (по умолчанию выключено). |

### 3.5. `drivers` (`PresetDrivers`)

Элементы массива `drivers.items` (`PresetDriverItem`) и список удаляемых OEM-драйверов `drivers.removePublishedNames`:

| Поле | Тип | По умолчанию | Описание |
| :--- | :--- | :--- | :--- |
| `path` | `string` | `""` | Путь к `.inf`-файлу или папке с драйверами. |
| `isDirectory` | `boolean \| null` | `null` | Необязательно: если опущено, при аудите и импорте определяется автоматически (`!path.toLowerCase().endsWith(".inf")`). |
| `recurse` | `boolean` | `true` | Флаг `/Recurse` (применяется DISM только если `isDirectory == true`). |
| `forceUnsigned` | `boolean` | `false` | Флаг `/ForceUnsigned`. |
| `injectToInstallWim` | `boolean` | `true` | Инъекция в целевой `install.wim`. |
| `injectToBootWim` | `boolean` | `false` | Инъекция в `sources\boot.wim` (**Index 2 — Windows Setup**, только при `target == "iso"`). |
| `removePublishedNames` | `string[]` | `[]` | Список публикуемых имён OEM-драйверов (`oem<N>.inf`) для удаления из `install.wim` перед инъекцией новых. |

### 3.6. `registry` (`RegistryBuildPlan`)

| Поле | Тип | По умолчанию | Описание |
| :--- | :--- | :--- | :--- |
| `regFiles` | `{ path: string, enabled: boolean }[]` | `[]` | Упорядоченный список `.reg`-файлов и состояние их активности. Внутри каждого элемента оба поля (`path`, `enabled`) обязательны. |

### 3.7. `files` (`FilesBuildPlan`, опциональный раздел)

Раздел `files` и массив `files.items` по умолчанию пусты (`[]`). Внутри каждого элемента `FileInjectionItem` обязательны поля `sourcePath`, `isDirectory`, `target`, `destinationPath`:

| Поле | Тип | По умолчанию | Описание |
| :--- | :--- | :--- | :--- |
| `sourcePath` | `string` | `—` (обязательно) | Путь к исходному файлу или папке на диске. |
| `isDirectory` | `boolean` | `—` (обязательно) | Является ли источник каталогом. |
| `target` | `"installWim" \| "isoRoot"` | `—` (обязательно) | Целевой контейнер (`C:\` внутри `install.wim` или корень ISO). |
| `destinationPath` | `string` | `—` (обязательно) | Относительный путь назначения (`""` — корень контейнера). |
| `conflictPolicy` | `"overwrite" \| "skip"` | `"overwrite"` | Политика при совпадении пути существующего файла. |

### 3.8. `postSetup` (`PostSetupBuildPlan`, опциональный раздел)

| Поле | Тип | По умолчанию | Описание |
| :--- | :--- | :--- | :--- |
| `items` | `PostSetupItem[]` | `[]` | Упорядоченный список задач постустановки (`installer`, `script`, `winget`). |
| `firstLogonMode` | `"runOnce" \| "autounattend"` | `"runOnce"` | Способ запуска фазы `firstLogon` (`RunOnce` в `NTUSER.DAT` или `FirstLogonCommands` в `autounattend.xml`). |
| `cleanupTempFiles` | `boolean` | `true` | Удалять временные дистрибутивы `Payload` после выполнения. |
| `waitForNetwork` | `boolean` | `true` | Ожидать сеть и доступность `winget` перед установкой пакетов. |

Каждый элемент `PostSetupItem`: `id`, `kind` (`"installer" \| "script" \| "winget"`), `phase` (`"beforeLogon" \| "firstLogon"`), `enabled` (`true`), `sourcePath`, `args`, `wingetId`, `displayName`, `wingetSource` (`"winget" \| "msstore"`), `wingetScope` (`"default" \| "machine" \| "user"`).

### 3.9. `output` (`PresetOutput`)

| Поле | Тип | По умолчанию | Описание |
| :--- | :--- | :--- | :--- |
| `target` | `"iso" \| "image"` | `"iso"` | Итоговый контейнер (`Загрузочный ISO` или `Только install-образ`). |
| `installFormat` | `"wim" \| "esd"` | `"wim"` | Формат упакованного `install.*`. |
| `autounattendPath` | `string \| null` | `null` | Путь к файлу ответов `autounattend.xml` (для `target == "iso"`). |
| `addGenericEiCfg` | `boolean` | `false` | Генерировать ли `sources\ei.cfg` (`Retail / VL=0`). |
| `uefiNoPrompt` | `boolean` | `true` | Использовать `efisys_noprompt.bin` вместо `efisys.bin`. |
| `volumeLabel` | `string` | `"WIN11_CUSTOM"` | Метка тома ISO (при сборке очищается до 32 символов `[A-Za-z0-9_-]`). |

---

## 4. Аудит отсутствующих локальных путей (`PresetImportResult`)

При вызове `import_preset` функция `find_missing_preset_paths` проверяет наличие на диске всех локальных файлов и папок (`appxLicense` проверяется всегда, когда `licensePath` непустой) и возвращает объект `{ preset, missingPaths }`. Каждый элемент `MissingPresetPath` содержит поля `id`, `kind`, `expectedDirectory`, `path`, `parentPath`.

| `kind` (`MissingPathKind`) | `expectedDirectory` | Источник в пресете |
| :--- | :---: | :--- |
| `appxPackage` | `false` | `apps.addPackages[].packagePath` |
| `appxDependency` | `false` | `apps.addPackages[].dependencyPaths[]` (`parentPath` = `packagePath`) |
| `appxLicense` | `false` | `apps.addPackages[].licensePath` (`parentPath` = `packagePath`) |
| `featuresSxs` | `true` | `features.defaultSxsPath`, `features.enable[].customSxsPath`, `features.capabilitiesAdd[].customSxsPath` (дедуплицировано регистронезависимо) |
| `updateItem` | `upd.isDirectory` | `updates.items[]` |
| `driverItem` | `isDirectory` или `!ends_with(".inf")` | `drivers.items[]` |
| `regFile` | `false` | `registry.regFiles[]` |
| `customFile` | `item.isDirectory` | `files.items[].sourcePath` |
| `postSetupFile` | `false` | `postSetup.items[].sourcePath` (`installer` / `script`) |
| `autounattendXml` | `false` | `output.autounattendPath` |

На экране **«Сборка и пресеты»** карточка `MissingPresetPathsCard` подсвечивает каждый отсутствующий элемент с кнопками **«Указать путь...»** (перепривязка через нативный диалог с повторным сканированием) и **«Удалить»**; список автоматически очищается по мере исправления или удаления соответствующих элементов в очередях.

---

## 5. Примеры пресетов

### 5.1. Минимальный пресет (ручная запись)

```json
{
  "schemaVersion": "1.0",
  "meta": {
    "name": "Clean Win11 ESD"
  },
  "apps": {
    "removePackages": [
      "Microsoft.BingNews_8wekyb3d8bbwe",
      "Microsoft.Clipchamp_yxz26nhyzhsrt"
    ]
  },
  "output": {
    "target": "image",
    "installFormat": "esd"
  }
}
```

### 5.2. Полный пресет

```json
{
  "schemaVersion": "1.0",
  "meta": {
    "name": "ISOCraft - Windows 11 Pro",
    "description": "Рабочий и игровой образ Windows 11 x64",
    "createdAt": "2026-11-20T03:00:00Z",
    "targetArch": "x64",
    "targetEditionHint": "Windows 11 Pro"
  },
  "apps": {
    "removePackages": [
      {
        "packageFamilyName": "Microsoft.BingNews_8wekyb3d8bbwe",
        "displayNameHint": "News"
      },
      "Microsoft.Clipchamp_yxz26nhyzhsrt"
    ],
    "addPackages": []
  },
  "features": {
    "enable": [
      {
        "name": "NetFx3",
        "enableAll": true,
        "customSxsPath": null
      }
    ],
    "disable": [],
    "capabilitiesAdd": [],
    "capabilitiesRemove": [],
    "defaultSxsPath": null
  },
  "services": {
    "changes": [
      {
        "name": "DiagTrack",
        "startType": "disabled"
      }
    ]
  },
  "updates": {
    "items": [
      {
        "path": "D:\\Win11_Assets\\Updates_24H2",
        "isDirectory": true
      }
    ],
    "runComponentCleanup": true,
    "cleanupResetBase": false
  },
  "drivers": {
    "items": [
      {
        "path": "D:\\Win11_Assets\\Drivers_NVMe",
        "isDirectory": true,
        "recurse": true,
        "forceUnsigned": false,
        "injectToInstallWim": true,
        "injectToBootWim": true
      }
    ],
    "removePublishedNames": ["oem5.inf"]
  },
  "registry": {
    "regFiles": [
      {
        "path": "D:\\Win11_Assets\\Tweaks\\privacy.reg",
        "enabled": true
      }
    ]
  },
  "files": {
    "items": [
      {
        "sourcePath": "D:\\Win11_Assets\\Scripts\\SetupComplete.cmd",
        "isDirectory": false,
        "target": "installWim",
        "destinationPath": "Windows\\Setup\\Scripts",
        "conflictPolicy": "overwrite"
      }
    ]
  },
  "postSetup": {
    "items": [
      {
        "id": "wg-7zip",
        "kind": "winget",
        "phase": "firstLogon",
        "enabled": true,
        "sourcePath": null,
        "args": "",
        "wingetId": "7zip.7zip",
        "displayName": "7-Zip",
        "wingetSource": "winget",
        "wingetScope": "machine"
      }
    ],
    "firstLogonMode": "runOnce",
    "cleanupTempFiles": true,
    "waitForNetwork": true
  },
  "output": {
    "target": "iso",
    "installFormat": "wim",
    "autounattendPath": "D:\\Win11_Assets\\autounattend.xml",
    "addGenericEiCfg": true,
    "uefiNoPrompt": true,
    "volumeLabel": "WIN11_CUSTOM"
  }
}
```
