import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { LogEntry } from "../types/studio";

const MAX_LOG_ENTRIES = 2000;
const BATCH_FLUSH_MS = 100;

let entries: LogEntry[] = [];
let seenIds = new Set<number>();
let pendingQueue: LogEntry[] = [];
let flushTimer: number | null = null;
let initialized = false;
let refCount = 0;
let unlistenFn: UnlistenFn | null = null;
let initToken = 0;

const listeners = new Set<() => void>();

function notifyListeners() {
  for (const listener of listeners) {
    listener();
  }
}

function appendUniqueEntries(batch: LogEntry[]): boolean {
  if (batch.length === 0) return false;
  let changed = false;
  const next = entries.slice();
  for (const item of batch) {
    if (seenIds.has(item.id)) continue;
    seenIds.add(item.id);
    next.push(item);
    changed = true;
  }
  if (!changed) return false;
  if (next.length > MAX_LOG_ENTRIES) {
    const dropped = next.length - MAX_LOG_ENTRIES;
    for (let i = 0; i < dropped; i += 1) {
      seenIds.delete(next[i].id);
    }
    entries = next.slice(dropped);
  } else {
    entries = next;
  }
  return true;
}

function flushPending() {
  if (flushTimer !== null) {
    window.clearTimeout(flushTimer);
    flushTimer = null;
  }
  if (pendingQueue.length === 0) return;
  const batch = pendingQueue;
  pendingQueue = [];
  if (appendUniqueEntries(batch)) {
    notifyListeners();
  }
}

function enqueueEntry(entry: LogEntry) {
  pendingQueue.push(entry);
  if (flushTimer === null) {
    flushTimer = window.setTimeout(flushPending, BATCH_FLUSH_MS);
  }
}

export async function initLogStore(): Promise<void> {
  refCount += 1;
  if (initialized) return;
  initialized = true;
  const myToken = ++initToken;

  listen<LogEntry>("app-log", (event) => {
    if (myToken !== initToken) return;
    enqueueEntry(event.payload);
  })
    .then((fn) => {
      if (myToken !== initToken || !initialized) {
        fn();
      } else {
        unlistenFn = fn;
      }
    })
    .catch(() => {});

  try {
    const initialLogs = await invoke<LogEntry[]>("get_logs");
    if (myToken !== initToken || !initialized) return;
    if (appendUniqueEntries(initialLogs)) {
      notifyListeners();
    }
    flushPending();
  } catch {
    // Backend log store may be unavailable in non-Tauri preview contexts
  }
}

export function destroyLogStore(): void {
  if (refCount > 0) {
    refCount -= 1;
  }
  if (refCount > 0) return;
  initialized = false;
  initToken += 1;
  if (flushTimer !== null) {
    window.clearTimeout(flushTimer);
    flushTimer = null;
  }
  pendingQueue = [];
  if (unlistenFn) {
    unlistenFn();
    unlistenFn = null;
  }
}

export function clearLogStore(): void {
  if (flushTimer !== null) {
    window.clearTimeout(flushTimer);
    flushTimer = null;
  }
  pendingQueue = [];
  entries = [];
  seenIds = new Set<number>();
  notifyListeners();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): LogEntry[] {
  return entries;
}

export function useLogs(): LogEntry[] {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

export function useLogCount(): number {
  return useSyncExternalStore(subscribe, () => entries.length, () => entries.length);
}
