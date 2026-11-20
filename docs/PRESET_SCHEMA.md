# Схема JSON-пресетов ISOCraft (версия `1.0`)

Пресет сохраняет конфигурацию сборки (намерения изменений и настройки вывода) в переносимом формате без привязки к временным путям рабочей директории. Импорт/экспорт выполняется командами `import_preset` и `export_preset` ([`preset.rs`](../src-tauri/crates/core/src/preset.rs)).

---

## 1. Структура верхнего уровня (`StudioPreset`)

```json
{
  "schemaVersion": "1.0",
  "meta": { },
  "apps": { },
  "features": { },
  "updates": { },
  "drivers": { },
  "registry": { },
  "files": { },
  "output": { }
}
```

- `schemaVersion`: строка, на текущей версии поддерживается строго `"1.0"` (при чтении автоматически удаляется UTF-8 BOM `\u{feff}`).

---

## 2. Описание разделов

### `meta`
- `name` (`string`) — название пресета.
- `description` (`string`) — произвольное описание.
- `createdAt` (`string`) — ISO-8601 UTC timestamp (заполняется автоматически при экспорте, если пустой).
- `targetArch` (`string`, по умолчанию `"x64"`) — целевая архитектура.
- `targetEditionHint` (`string | null`) — подсказка редакции (например, `"Windows 11 Pro"`).

### `apps`
- `removePackages`: массив элементов удаления Provisioned Appx. Поддерживаются два формата:
  - Объект: `{ "packageFamilyName": "...", "displayNameHint": "..." }` (рекомендуется для читаемости);
  - Строка: `"Microsoft.BingNews_8wekyb3d8bbwe"` (обратная совместимость).
  Во время сборки `PackageFamilyName` автоматически разрешается в актуальный `PackageName` открытого билда Windows 11.
- `addPackages`: массив объектов `AddAppxRequest`:
  - `packagePath` (`string`)
  - `dependencyPaths` (`string[]`)
  - `licensePath` (`string | null`)
  - `skipLicense` (`boolean`, по умолчанию `true`)
  - `regionAll` (`boolean`, по умолчанию `true`)
  - `stubOption`: `"None" | "UseStub" | "UsePreference"`

### `features`
- `enable`: массив `{ "name": string, "enableAll": boolean, "customSxsPath": string | null }`
- `disable`: массив системных имён `string[]`
- `capabilitiesAdd`: массив `{ "name": string, "limitAccess": boolean, "customSxsPath": string | null }`
- `capabilitiesRemove`: массив имён `string[]`
- `defaultSxsPath` (`string | null`)

### `updates`
- `items`: массив `{ "path": string, "isDirectory": boolean }`
- `runComponentCleanup` (`boolean`)
- `cleanupResetBase` (`boolean`, по умолчанию `false`)

### `drivers`
- `items`: массив `{ "path": string, "isDirectory"?: boolean, "recurse": boolean, "forceUnsigned": boolean, "injectToInstallWim": boolean, "injectToBootWim": boolean }`. Если `isDirectory` опущен (например, при ручном создании пресета), тип определяется автоматически по окончанию `.inf`.

### `registry`
- `regFiles`: упорядоченный массив `{ "path": string, "enabled": boolean }`

### `files` *(опциональный раздел)*
- `items`: массив правил копирования раздела «Файлы»:
  - `sourcePath` (`string`)
  - `isDirectory` (`boolean`)
  - `target`: `"installWim" | "isoRoot"`
  - `destinationPath` (`string`, относительный путь внутри `C:\` или корня ISO; `""` — корень)
  - `conflictPolicy`: `"overwrite" | "skip"`

### `output`
- `target`: `"iso" | "image"` (при загрузке ISO-пресета поверх открытого `.wim` или `.esd` автоматически приводится к `"image"`, а `injectToBootWim` сбрасывается)
- `installFormat`: `"wim" | "esd" | "swm"`
- `splitSizeMb` (`number`, по умолчанию `3800`)
- `autounattendPath` (`string | null`)
- `addGenericEiCfg` (`boolean`)
- `uefiNoPrompt` (`boolean`, по умолчанию `true`)
- `volumeLabel` (`string`, по умолчанию `"WIN11_CUSTOM"`)

---

## 3. Проверка отсутствующих локальных путей при импорте

При вызове `import_preset` функция `find_missing_preset_paths` проверяет существование на диске всех локальных файлов и папок из загруженного пресета и возвращает список `MissingPresetPath` с типами (`MissingPathKind`):

| `kind` | Проверяемый тип | Источник |
| :--- | :--- | :--- |
| `appxPackage` | файл | `apps.addPackages[].packagePath` |
| `appxDependency` | файл | `apps.addPackages[].dependencyPaths[]` |
| `appxLicense` | файл | `apps.addPackages[].licensePath` |
| `featuresSxs` | папка | `features.defaultSxsPath` и `customSxsPath` (дедуплицировано) |
| `updateItem` | файл или папка | `updates.items[]` |
| `driverItem` | файл или папка | `drivers.items[]` |
| `regFile` | файл | `registry.regFiles[]` |
| `customFile` | файл или папка | `files.items[].sourcePath` |
| `autounattendXml` | файл | `output.autounattendPath` |

В UI на экране **«Сборка и пресеты»** отсутствующие пути подсвечиваются отдельной карточкой с кнопками **«Указать путь...»** (перепривязка через системный диалог с повторным сканированием) и **«Удалить»**.

---

## 4. Полный пример пресета

```json
{
  "schemaVersion": "1.0",
  "meta": {
    "name": "ISOCraft - Windows 11 Pro",
    "description": "Минимальный игровой и рабочий образ Win11 x64",
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
    ]
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
  "output": {
    "target": "iso",
    "installFormat": "wim",
    "splitSizeMb": 3800,
    "autounattendPath": "D:\\Win11_Assets\\autounattend.xml",
    "addGenericEiCfg": true,
    "uefiNoPrompt": true,
    "volumeLabel": "WIN11_CUSTOM"
  }
}
```
