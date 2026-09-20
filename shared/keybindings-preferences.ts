import keybindingsPreferences from "./keybindings-preferences.cjs";

export type {
  KeybindingCommandDef,
  KeybindingScope,
} from "./keybindings-preferences.cjs";

export const {
  DEFAULT_MAX_BINDINGS_PER_COMMAND,
  KEYBINDING_COMMANDS,
  KEYBINDING_COMMAND_IDS,
  KEYBINDING_SCOPES,
  defaultKeybindings,
  normalizeBinding,
  normalizeKeybindings,
  mergeKeybindings,
  getEffectiveKeybindings,
  findKeybindingConflicts,
} = keybindingsPreferences;
