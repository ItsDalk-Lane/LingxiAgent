export type KeybindingScope = "global" | "app" | "local";

export interface KeybindingCommandDef {
  id: string;
  defaultKeys: string[];
  scope: KeybindingScope;
}

export const DEFAULT_MAX_BINDINGS_PER_COMMAND: number;
export const KEYBINDING_COMMANDS: KeybindingCommandDef[];
export const KEYBINDING_COMMAND_IDS: string[];
export const KEYBINDING_SCOPES: KeybindingScope[];
export function defaultKeybindings(): Record<string, string[]>;
export function normalizeBinding(value?: unknown): string;
export function normalizeKeybindings(value?: unknown): Record<string, string[]>;
export function mergeKeybindings(stored?: unknown, patch?: unknown): Record<string, string[]>;
export function getEffectiveKeybindings(stored?: unknown): Record<string, string[]>;
export function findKeybindingConflicts(commandId: string, bindings: string | string[], stored?: unknown): string[];
