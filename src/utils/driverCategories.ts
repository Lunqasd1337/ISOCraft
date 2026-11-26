export const DRIVER_CATEGORY_KEYS = {
  all: "screens.drivers.categories.all",
  storage: "screens.drivers.categories.storage",
  network: "screens.drivers.categories.network",
  display: "screens.drivers.categories.display",
  audio: "screens.drivers.categories.audio",
  usb: "screens.drivers.categories.usb",
  bluetooth: "screens.drivers.categories.bluetooth",
  input: "screens.drivers.categories.input",
  other: "screens.drivers.categories.other",
} as const;

export type DriverCategoryKey =
  (typeof DRIVER_CATEGORY_KEYS)[keyof typeof DRIVER_CATEGORY_KEYS];

export function driverCategoryLabelKey(category: string): DriverCategoryKey {
  return category in DRIVER_CATEGORY_KEYS
    ? DRIVER_CATEGORY_KEYS[category as keyof typeof DRIVER_CATEGORY_KEYS]
    : DRIVER_CATEGORY_KEYS.other;
}
