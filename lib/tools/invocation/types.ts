export type ToolOrigin = "first-party" | "plugin" | "mcp";

export type ToolInvocationRoute =
  | "direct"
  | "deferred"
  | "plugin-dev-chat"
  | "plugin-dev-http"
  | "isolated";

export type ToolTargetId = string & { readonly __toolTargetId: unique symbol };

export interface ToolTargetIdentity {
  readonly targetId: ToolTargetId;
  readonly origin: ToolOrigin;
  readonly sourceId: string;
  readonly localName: string;
  readonly publicName: string;
  readonly capabilityBase: string;
}
