/**
 * ContentBlock 类型化渲染注册表。
 *
 * 注册表必须覆盖联合类型中的每一种内容块；新增类型若没有同步提供渲染器，
 * TypeScript 会在注册处直接报错，而不是等运行时落进中央 switch 的 default。
 */
import { createElement, type ComponentType, type ReactNode } from 'react';
import type { ContentBlock } from '../../stores/chat-types';

export type ContentBlockType = ContentBlock['type'];
export type ContentBlockOf<Type extends ContentBlockType> = Extract<ContentBlock, { type: Type }>;

export interface BlockRendererContext {
  agentName: string;
  agentId?: string | null;
  yuan: string;
  sessionPath: string;
  messageId: string;
  blockIdx: number;
  isStreaming: boolean;
  readOnly: boolean;
  skillPrompt: string | null;
}

export type BlockRendererProps<Type extends ContentBlockType> = BlockRendererContext & {
  block: ContentBlockOf<Type>;
};

export type ContentBlockRendererRegistry = {
  [Type in ContentBlockType]: ComponentType<BlockRendererProps<Type>>;
};

let registry: ContentBlockRendererRegistry | null = null;

export function registerBlockRenderers(next: ContentBlockRendererRegistry): void {
  registry = next;
}

export function renderRegisteredContentBlock(
  block: ContentBlock,
  context: BlockRendererContext,
): ReactNode {
  if (!registry) return null;
  // 具体键与具体 block 的对应关系已由 registerBlockRenderers 的映射类型校验；
  // 这里是在联合类型运行时分发点做唯一一次收窄。
  const Renderer = registry[block.type] as ComponentType<BlockRendererContext & { block: ContentBlock }>;
  return createElement(Renderer, { ...context, block });
}

export function registeredContentBlockTypesForTests(): ContentBlockType[] {
  return registry ? Object.keys(registry) as ContentBlockType[] : [];
}
