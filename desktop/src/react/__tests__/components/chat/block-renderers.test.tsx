// @vitest-environment jsdom

import { describe, expect, it } from 'vitest';
import '../../../components/chat/AssistantMessage';
import { registeredContentBlockTypesForTests } from '../../../components/chat/block-renderers';

describe('typed content block renderer registry', () => {
  it('为每一种 ContentBlock 提供显式渲染器', () => {
    expect(registeredContentBlockTypesForTests().sort()).toEqual([
      'artifact',
      'cron_confirm',
      'file',
      'interactive_card',
      'interlude',
      'media_generation',
      'mood',
      'plugin_card',
      'screenshot',
      'session_confirmation',
      'settings_confirm',
      'settings_update',
      'skill',
      'subagent',
      'suggestion_card',
      'text',
      'thinking',
      'tool_group',
      'turn_status',
      'workflow',
    ]);
  });
});
