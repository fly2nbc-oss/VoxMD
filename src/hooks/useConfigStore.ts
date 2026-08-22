import { Store } from "@tauri-apps/plugin-store";
import { useCallback, useEffect, useRef, useState } from "react";
import { defaultConfig } from "../defaults";
import { CONFIG_KEY, mergeConfig, QUEUE_KEY, STORE_FILE } from "../lib/configStore";
import { toMsg } from "../lib/jobs";
import type { AppConfig, QueueItem } from "../types";

export interface ConfigStore {
  config: AppConfig;
  /** What is on disk. Differs from `config` exactly while edits are unsaved. */
  saved: AppConfig;
  /** Live edit, not persisted until `save` (or a `persist` call) runs. */
  setConfig: (next: AppConfig | ((prev: AppConfig) => AppConfig)) => void;
  /** Write to disk. Accepts an updater so callers running after an `await`
   *  cannot resurrect a value captured before it. */
  persist: (update: AppConfig | ((prev: AppConfig) => AppConfig)) => Promise<void>;
  /** Drop unsaved edits, e.g. when the settings drawer is dismissed. */
  revert: () => void;
  loadQueue: () => Promise<unknown>;
  saveQueue: (items: QueueItem[]) => Promise<void>;
  ready: boolean;
  loadError: string;
}

export function useConfigStore(): ConfigStore {
  const [config, setConfigState] = useState<AppConfig>(defaultConfig);
  const [saved, setSaved] = useState<AppConfig>(defaultConfig);
  const [ready, setReady] = useState(false);
  const [loadError, setLoadError] = useState("");

  const storeRef = useRef<Awaited<ReturnType<typeof Store.load>> | null>(null);
  /** Mirrors `config` so async callers read the current value. */
  const currentRef = useRef(config);
  /** Last value written to disk, used to discard unsaved edits. */
  const savedRef = useRef(config);

  const setConfig = useCallback((next: AppConfig | ((prev: AppConfig) => AppConfig)) => {
    setConfigState((prev) => {
      const value = typeof next === "function" ? next(prev) : next;
      currentRef.current = value;
      return value;
    });
  }, []);

  useEffect(() => {
    (async () => {
      try {
        const store = await Store.load(STORE_FILE, { autoSave: true, defaults: {} });
        storeRef.current = store;
        const saved = await store.get<AppConfig>(CONFIG_KEY);
        const merged = mergeConfig(saved ?? undefined);
        currentRef.current = merged;
        savedRef.current = merged;
        setSaved(merged);
        setConfigState(merged);
      } catch (e) {
        // Surfaced rather than silently reverting to defaults, since the next
        // save would otherwise overwrite the file still on disk.
        setLoadError(toMsg(e));
      } finally {
        setReady(true);
      }
    })();
  }, []);

  const persist = useCallback(async (update: AppConfig | ((prev: AppConfig) => AppConfig)) => {
    const next = typeof update === "function" ? update(currentRef.current) : update;
    currentRef.current = next;
    savedRef.current = next;
    setSaved(next);
    setConfigState(next);
    const store =
      storeRef.current ?? (await Store.load(STORE_FILE, { autoSave: true, defaults: {} }));
    await store.set(CONFIG_KEY, next);
    await store.save();
  }, []);

  const revert = useCallback(() => {
    currentRef.current = savedRef.current;
    setConfigState(savedRef.current);
  }, []);

  const loadQueue = useCallback(async () => {
    const store =
      storeRef.current ?? (await Store.load(STORE_FILE, { autoSave: true, defaults: {} }));
    return store.get(QUEUE_KEY);
  }, []);

  const saveQueue = useCallback(async (items: QueueItem[]) => {
    const store =
      storeRef.current ?? (await Store.load(STORE_FILE, { autoSave: true, defaults: {} }));
    await store.set(QUEUE_KEY, items);
    await store.save();
  }, []);

  return { config, saved, setConfig, persist, revert, loadQueue, saveQueue, ready, loadError };
}
