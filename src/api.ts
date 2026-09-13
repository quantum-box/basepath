import { invoke, isTauri as isTauriRuntime } from "@tauri-apps/api/core";
import type { Goal, Initiative, Task } from "./data";

export type PersistedState = {
  goals: Goal[];
  tasks: Task[];
  initiatives: Initiative[];
  nextDone: Record<string, boolean>;
  reflection: string;
  savedReflection: string;
  activity: string[];
  selectedId: string;
};

export function isTauri(): boolean {
  return isTauriRuntime();
}

export async function loadState(): Promise<PersistedState | null> {
  if (!isTauri()) return null;
  return invoke<PersistedState | null>("load_state");
}

export async function saveState(state: PersistedState): Promise<void> {
  if (!isTauri()) return;
  await invoke("save_state", { state });
}
