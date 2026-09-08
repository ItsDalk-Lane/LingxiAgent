import type { Extensions } from '@tiptap/core';
import StarterKit from '@tiptap/starter-kit';
import { Bold } from '@tiptap/extension-bold';
import Placeholder, { type PlaceholderOptions } from '@tiptap/extension-placeholder';
import { SkillBadge } from './extensions/skill-badge';
import { FileBadge } from './extensions/file-badge';
import { SessionBadge } from './extensions/session-badge';
import { AgentBadge } from './extensions/agent-badge';

export type InputEditorPlaceholder = PlaceholderOptions['placeholder'];

const ChatInputBold = Bold.extend({
  inclusive: false,
  keepOnSplit: false,
});

export function createInputEditorExtensions(placeholder: InputEditorPlaceholder): Extensions {
  return [
    StarterKit.configure({
      heading: false,
      blockquote: false,
      codeBlock: false,
      horizontalRule: false,
      dropcursor: false,
      gapcursor: false,
      link: false,
      bold: false,
      // StarterKit 3 默认会在列表等块后自动补一个无法标明来源的空 paragraph。
      // 输入框把空段视为用户正文，因此关闭该插件，避免序列化时猜测删除尾行。
      trailingNode: false,
    }),
    ChatInputBold,
    Placeholder.configure({ placeholder }),
    SkillBadge,
    FileBadge,
    SessionBadge,
    AgentBadge,
  ];
}
