import { describe, it, expect, vi } from "vitest";

import {
  AGENT_REVIEW_RECORD_TYPE,
  MESSAGE_ORIGIN_RECORD_TYPE,
  MESSAGE_PRESENTATION_RECORD_TYPE,
  abortPendingDesktopSubmission,
  submitDesktopSessionInterjection,
  submitDesktopSessionMessage,
  submitDesktopSessionMessageWithReceipt,
} from "../core/desktop-session-submit.ts";
import fs from "fs";
import os from "os";
import path from "path";

function makeFakeSession({ replyText = "desktop reply", toolMedia = [], toolMediaDetails = null, settingsUpdate = null }: any = {}) {
  const subs = [];
  return {
    subscribe: (fn) => {
      subs.push(fn);
      return () => {
        const idx = subs.indexOf(fn);
        if (idx >= 0) subs.splice(idx, 1);
      };
    },
    prompt: vi.fn<(...args: any[]) => Promise<any>>(async () => {
      for (const fn of subs) {
        fn({ type: "message_update", assistantMessageEvent: { type: "text_delta", delta: replyText } });
        if (toolMediaDetails) {
          fn({
            type: "tool_execution_end",
            isError: false,
            result: { details: { media: toolMediaDetails } },
          });
        }
        for (const url of toolMedia) {
          fn({
            type: "tool_execution_end",
            isError: false,
            result: { details: { media: { mediaUrls: [url] } } },
          });
        }
        if (settingsUpdate) {
          fn({
            type: "tool_execution_end",
            isError: false,
            result: { details: { settingsUpdate } },
          });
        }
      }
    }),
    model: null,
  };
}

function sessionFileMarker({ fileId, sessionPath, sessionId = undefined, label, kind = "attachment" }) {
  return `[SessionFile] ${JSON.stringify({
    fileId,
    sessionPath,
    ...(sessionId ? { sessionId } : {}),
    label,
    kind,
  })}`;
}

describe("submitDesktopSessionMessage", () => {
  it("rejects a sessionId/sessionPath mismatch before loading or emitting (#2078)", async () => {
    const engine = {
      getSessionManifest: vi.fn(() => ({ currentLocator: { path: "/tmp/canonical.jsonl" } })),
      ensureSessionLoaded: vi.fn(),
      promptSession: vi.fn(),
    };

    await expect(submitDesktopSessionMessage(engine, {
      sessionId: "sess_target",
      sessionPath: "/tmp/other.jsonl",
      text: "hello",
    })).rejects.toThrow("session identity mismatch");
    expect(engine.ensureSessionLoaded).not.toHaveBeenCalled();
    expect(engine.promptSession).not.toHaveBeenCalled();
  });
  it("rejects concurrent submissions for the same session before streaming status is emitted", async () => {
    const session = makeFakeSession();
    const ready = (Promise as any).withResolvers();
    const engine = {
      ensureSessionLoaded: vi.fn(() => ready.promise),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
      isSessionStreaming: vi.fn(() => false),
    };

    const first = submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "first",
      displayMessage: { text: "first" },
    });
    await Promise.resolve();

    await expect(submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "second",
      displayMessage: { text: "second" },
    })).rejects.toThrow("session_busy");

    ready.resolve(session);
    await expect(first).resolves.toMatchObject({ text: "desktop reply" });
    expect(engine.emitEvent).toHaveBeenCalledWith(
      expect.objectContaining({ type: "session_status", isStreaming: true }),
      "/tmp/desk.jsonl",
    );
  });

  it("rejects concurrent submissions for moved paths with the same session id", async () => {
    const session = makeFakeSession();
    const ready = (Promise as any).withResolvers();
    const originalPath = "/tmp/original-desk.jsonl";
    const movedPath = "/tmp/archived/renamed-desk.jsonl";
    const sessionId = "sess_desktop_submit";
    const engine = {
      getSessionIdForPath: vi.fn((sessionPath) => (
        sessionPath === originalPath || sessionPath === movedPath ? sessionId : null
      )),
      ensureSessionLoaded: vi.fn((sessionPath) => (
        sessionPath === originalPath ? ready.promise : Promise.resolve(session)
      )),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
      isSessionStreaming: vi.fn(() => false),
    };

    const first = submitDesktopSessionMessage(engine, {
      sessionPath: originalPath,
      text: "first",
      displayMessage: { text: "first" },
    });
    await Promise.resolve();

    await expect(submitDesktopSessionMessage(engine, {
      sessionPath: movedPath,
      text: "second",
      displayMessage: { text: "second" },
    })).rejects.toThrow("session_busy");

    ready.resolve(session);
    await expect(first).resolves.toMatchObject({ text: "desktop reply" });
  });

  it("emits a session-scoped user message, toggles streaming status, and returns captured assistant output", async () => {
    const session = makeFakeSession({
      replyText: "desktop reply",
      toolMedia: ["https://example.com/a.png"],
    });
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };
    const onDelta = vi.fn();

    const result = await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "hello from bridge",
      displayMessage: { text: "hello from bridge" },
      uiContext: null,
      onDelta,
    });

    expect(engine.ensureSessionLoaded).toHaveBeenCalledWith("/tmp/desk.jsonl");
    expect(engine.setUiContext).toHaveBeenCalledWith("/tmp/desk.jsonl", null);
    expect(engine.emitEvent).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({ type: "session_status", isStreaming: true }),
      "/tmp/desk.jsonl",
    );
    // 发送即置忙（第 1 次）之后，afterCachePreflight 幂等再置忙并投影用户消息。
    expect(engine.emitEvent).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({ type: "session_status", isStreaming: true }),
      "/tmp/desk.jsonl",
    );
    expect(engine.emitEvent).toHaveBeenNthCalledWith(
      3,
      expect.objectContaining({
        type: "session_user_message",
        message: expect.objectContaining({ text: "hello from bridge" }),
      }),
      "/tmp/desk.jsonl",
    );
    expect(engine.promptSession).toHaveBeenCalledWith("/tmp/desk.jsonl", "hello from bridge", undefined);
    expect(onDelta).toHaveBeenCalledWith("desktop reply", "desktop reply");
    expect(engine.emitEvent).toHaveBeenLastCalledWith(
      expect.objectContaining({ type: "session_status", isStreaming: false }),
      "/tmp/desk.jsonl",
    );
    expect(result).toEqual({
      text: "desktop reply",
      toolMedia: [{ type: "remote_url", url: "https://example.com/a.png" }],
    });
  });

  it("can replay a hidden model input without emitting a visible user projection", async () => {
    const session = makeFakeSession({ replyText: "background reply" });
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };
    const hiddenInput = '<hana-background-result task-id="task-1">done</hana-background-result>';

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: hiddenInput,
      projectUserMessage: false,
    });

    expect(engine.promptSession).toHaveBeenCalledWith("/tmp/desk.jsonl", hiddenInput, undefined);
    expect(engine.emitEvent.mock.calls.some(([event]) => event?.type === "session_user_message")).toBe(false);
    expect(engine.emitEvent).toHaveBeenCalledWith(
      expect.objectContaining({ type: "session_status", isStreaming: true }),
      "/tmp/desk.jsonl",
    );
  });

  it("deduplicates SessionFile refs by stable sessionId when it is available", async () => {
    const session = makeFakeSession();
    const engine = {
      getSessionIdForPath: vi.fn(() => "sess_submit_stable"),
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "open it",
      displayMessage: { text: "open it" },
      sessionFileRefs: [
        {
          fileId: "sf_note",
          sessionId: "sess_submit_stable",
          sessionPath: "/tmp/old-location.jsonl",
          label: "old note",
          kind: "attachment",
        },
        {
          fileId: "sf_note",
          sessionId: "sess_submit_stable",
          sessionPath: "/tmp/new-location.jsonl",
          label: "new note",
          kind: "attachment",
        },
      ],
    });

    expect(engine.promptSession).toHaveBeenCalledWith(
      "/tmp/desk.jsonl",
      `${sessionFileMarker({
        fileId: "sf_note",
        sessionId: "sess_submit_stable",
        sessionPath: "/tmp/old-location.jsonl",
        label: "old note",
      })}\nopen it`,
      undefined,
    );
  });

  it("preserves an existing prompt envelope without duplicating media, SessionFile, or reminder markers", async () => {
    const session = makeFakeSession();
    (session as any).sessionManager = { appendCustomEntry: vi.fn() };
    const registerSessionFile = vi.fn();
    const engine = {
      getSessionManifest: vi.fn(() => ({ currentLocator: { path: "/tmp/desk.jsonl" } })),
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
      registerSessionFile,
      renderSessionReminderBlock: vi.fn(() => ({
        block: "[hana_reminder]\nnew reminder\n[/hana_reminder]",
        receipt: { throughSeq: 1 },
      })),
    };
    const originalPrompt = [
      '[SessionFile] {"fileId":"sf-1","sessionId":"sess-1","sessionPath":"/tmp/desk.jsonl","label":"note","kind":"attachment"}',
      "[attached_image: /tmp/image.png]",
      "review this",
    ].join("\n");

    await submitDesktopSessionMessage(engine, {
      sessionId: "sess-1",
      sessionPath: "/tmp/desk.jsonl",
      text: originalPrompt,
      images: [{ type: "image", data: "BASE64", mimeType: "image/png" }],
      imageAttachmentPaths: ["/tmp/image.png"],
      sessionFileRefs: [{
        fileId: "sf-1",
        sessionId: "sess-1",
        sessionPath: "/tmp/desk.jsonl",
        label: "note",
        kind: "attachment",
      }],
      displayMessage: {
        text: "review this",
        attachments: [{ fileId: "sf-1", path: "/tmp/image.png", name: "image.png" }],
      },
      preservePromptEnvelope: true,
    } as any);

    expect(engine.promptSession).toHaveBeenCalledWith(
      "/tmp/desk.jsonl",
      originalPrompt,
      {
        images: [{ type: "image", data: "BASE64", mimeType: "image/png" }],
        imageAttachmentPaths: ["/tmp/image.png"],
      },
    );
    expect(registerSessionFile).not.toHaveBeenCalled();
    expect(engine.renderSessionReminderBlock).not.toHaveBeenCalled();
  });

  it("threads clientMessageId into the session user message event", async () => {
    const session = makeFakeSession();
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "hello",
      clientMessageId: "client-user-1",
      displayMessage: { text: "hello" },
    } as any);

    expect(engine.emitEvent).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "session_user_message",
        clientMessageId: "client-user-1",
        message: expect.objectContaining({ text: "hello" }),
      }),
      "/tmp/desk.jsonl",
    );
  });

  it("forwards turn context to promptSession without exposing it in the visible user message", async () => {
    const session = makeFakeSession();
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "hello",
      displayMessage: { text: "hello" },
      context: {
        beforeUser: "world lore",
        metadata: { pluginId: "tavern" },
      },
    } as any);

    expect(engine.emitEvent).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "session_user_message",
        message: expect.objectContaining({ text: "hello" }),
      }),
      "/tmp/desk.jsonl",
    );
    expect(engine.emitEvent).not.toHaveBeenCalledWith(
      expect.objectContaining({
        type: "session_user_message",
        message: expect.objectContaining({ text: expect.stringContaining("world lore") }),
      }),
      expect.anything(),
    );
    expect(engine.promptSession).toHaveBeenCalledWith(
      "/tmp/desk.jsonl",
      "hello",
      { context: { beforeUser: "world lore", metadata: { pluginId: "tavern" } } },
    );
  });

  it("prefers structured tool media items over legacy mediaUrls", async () => {
    const item = { type: "session_file", fileId: "sf_1", filePath: "/tmp/a.png" };
    const session = makeFakeSession({
      replyText: "desktop reply",
      toolMediaDetails: { items: [item], mediaUrls: ["/tmp/a.png"] },
    });
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    const result = await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "hello",
      displayMessage: { text: "hello" },
    });

    expect(result.toolMedia).toEqual([item]);
  });

  it("appends settings update summaries into captured bridge text", async () => {
    const session = makeFakeSession({
      replyText: "",
      settingsUpdate: {
        status: "applied",
        action: "core.apply",
        key: "locale",
        title: "Locale updated",
        summary: "Locale changed.",
        changes: [{ key: "locale", label: "Locale", before: "zh-CN", after: "en" }],
      },
    });
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    const result = await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "change locale",
      displayMessage: { text: "change locale" },
    });

    expect(result.text).toContain("Locale updated");
    expect(result.text).toContain("Locale: zh-CN -> en");
  });

  // ── #1610: /rc 来源元信息持久化 ──

  it("persists a message-origin custom entry before prompting for bridge_rc submissions", async () => {
    const session = makeFakeSession();
    const appendOrder: string[] = [];
    (session as any).sessionManager = {
      appendCustomEntry: vi.fn(() => {
        appendOrder.push("origin-entry");
      }),
    };
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => {
        appendOrder.push("prompt");
        return session.prompt(text, opts);
      }),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "hello from telegram",
      displayMessage: {
        text: "hello from telegram",
        source: "bridge_rc",
        bridgeSessionKey: "telegram:12345",
      },
    });

    expect((session as any).sessionManager.appendCustomEntry).toHaveBeenCalledWith(
      MESSAGE_ORIGIN_RECORD_TYPE,
      expect.objectContaining({
        source: "bridge_rc",
        bridgeSessionKey: "telegram:12345",
      }),
    );
    // 来源记录必须先于 prompt 写入，让条目紧邻它注释的 user message
    expect(appendOrder).toEqual(["origin-entry", "prompt"]);
  });

  it("does not write a message-origin entry for plain desktop submissions", async () => {
    const session = makeFakeSession();
    (session as any).sessionManager = { appendCustomEntry: vi.fn() };
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "hello",
      displayMessage: { text: "hello" },
    });

    expect((session as any).sessionManager.appendCustomEntry).not.toHaveBeenCalled();
  });

  it("persists a skills-only presentation even when its visible text matches the prompt", async () => {
    const session = makeFakeSession();
    const appendCustomEntry = vi.fn();
    Object.assign(session, { sessionManager: { appendCustomEntry } });
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "你好",
      displayMessage: {
        text: "你好",
        skills: ["persist-test-skill"],
      },
    });

    expect(appendCustomEntry).toHaveBeenCalledWith(
      MESSAGE_PRESENTATION_RECORD_TYPE,
      expect.objectContaining({
        displayText: "你好",
        skills: ["persist-test-skill"],
      }),
    );
  });

  it("persists review presentation and result as message-level custom entries", async () => {
    const session = makeFakeSession();
    const appendCustomEntry = vi.fn();
    (session as any).sessionManager = { appendCustomEntry };
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (_sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "user request\n\n[另一位 Agent 的审阅结果]\nfindings",
      displayMessage: {
        text: "user request @Critic",
        skills: ["persist-test-skill"],
        agentMentions: [{ agentId: "critic", label: "Critic" }],
        agentReview: {
          requestId: "review-1",
          status: "completed",
          reviewedSessionId: "sess_parent",
          reviewerSessionId: "sess_review",
          reviewerAgentId: "critic",
          reviewerAgentName: "Critic",
          text: "findings",
        },
      },
    });

    expect(appendCustomEntry).toHaveBeenNthCalledWith(1, MESSAGE_PRESENTATION_RECORD_TYPE, expect.objectContaining({
      displayText: "user request @Critic",
      skills: ["persist-test-skill"],
      agentMentions: [{ agentId: "critic", label: "Critic" }],
    }));
    expect(appendCustomEntry).toHaveBeenNthCalledWith(2, AGENT_REVIEW_RECORD_TYPE, expect.objectContaining({
      reviewedSessionId: "sess_parent",
      reviewerSessionId: "sess_review",
      text: "findings",
    }));
  });

  it("still submits the message when the origin entry write fails", async () => {
    const session = makeFakeSession();
    (session as any).sessionManager = {
      appendCustomEntry: vi.fn(() => {
        throw new Error("disk hiccup");
      }),
    };
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    const result = await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "hello from qq",
      displayMessage: { text: "hello from qq", source: "bridge_rc", bridgeSessionKey: "qq:678" },
    });

    expect(result.text).toBe("desktop reply");
  });

  it("still emits session_status=false when promptSession throws", async () => {
    const session = makeFakeSession();
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async () => {
        throw new Error("boom");
      }),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    await expect(submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "hello",
      displayMessage: { text: "hello" },
    })).rejects.toThrow("boom");

    expect(engine.emitEvent).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({ type: "session_status", isStreaming: true }),
      "/tmp/desk.jsonl",
    );
    expect(engine.emitEvent).toHaveBeenLastCalledWith(
      expect.objectContaining({ type: "session_status", isStreaming: false }),
      "/tmp/desk.jsonl",
    );
  });

  it("forwards image attachment paths to promptSession", async () => {
    const session = makeFakeSession();
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "see image",
      images: [{ type: "image", data: "BASE64", mimeType: "image/png" }],
      imageAttachmentPaths: ["/tmp/upload.png"],
      displayMessage: { text: "see image" },
    });

    expect(engine.promptSession).toHaveBeenCalledWith(
      "/tmp/desk.jsonl",
      "[attached_image: /tmp/upload.png]\nsee image",
      {
        images: [{ type: "image", data: "BASE64", mimeType: "image/png" }],
        imageAttachmentPaths: ["/tmp/upload.png"],
      },
    );
  });

  it("forwards videos to promptSession and records attached video markers", async () => {
    const session = makeFakeSession();
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "see video",
      videos: [{ type: "video", data: "BASE64", mimeType: "video/mp4" }],
      videoAttachmentPaths: ["/tmp/upload.mp4"],
      displayMessage: { text: "see video" },
    });

    expect(engine.promptSession).toHaveBeenCalledWith(
      "/tmp/desk.jsonl",
      "[attached_video: /tmp/upload.mp4]\nsee video",
      {
        videos: [{ type: "video", data: "BASE64", mimeType: "video/mp4" }],
        videoAttachmentPaths: ["/tmp/upload.mp4"],
      },
    );
  });

  it("forwards audios to promptSession and records attached audio markers", async () => {
    const session = makeFakeSession();
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "hear audio",
      audios: [{ type: "audio", data: "BASE64", mimeType: "audio/wav" }],
      audioAttachmentPaths: ["/tmp/upload.wav"],
      displayMessage: { text: "hear audio" },
    });

    expect(engine.promptSession).toHaveBeenCalledWith(
      "/tmp/desk.jsonl",
      "[attached_audio: /tmp/upload.wav]\nhear audio",
      {
        audios: [{ type: "audio", data: "BASE64", mimeType: "audio/wav" }],
        audioAttachmentPaths: ["/tmp/upload.wav"],
      },
    );
  });

  it("adds SessionFile references for display-only audio attachments", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-display-audio-"));
    try {
      const filePath = path.join(tmpDir, "voice.wav");
      fs.writeFileSync(filePath, Buffer.from([0x52, 0x49, 0x46, 0x46]));
      const session = makeFakeSession();
      const sessionPath = path.join(tmpDir, "main.jsonl");
      fs.writeFileSync(sessionPath, "{}\n");
      const registerSessionFile = vi.fn(({ sessionPath, filePath, label, origin, storageKind }) => ({
        id: "sf_audio_attachment",
        fileId: "sf_audio_attachment",
        sessionPath,
        filePath,
        realPath: filePath,
        displayName: label,
        filename: path.basename(filePath),
        label,
        ext: "wav",
        mime: "audio/wav",
        size: 4,
        kind: "audio",
        origin,
        storageKind,
        createdAt: 1,
      }));
      const queueVoiceTranscription = vi.fn();
      const engine = {
        lingxiHome: tmpDir,
        registerSessionFile,
        speechRecognition: { queueVoiceTranscription },
        ensureSessionLoaded: vi.fn(async () => session),
        promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
        emitEvent: vi.fn(),
        setUiContext: vi.fn(),
      };

      await submitDesktopSessionMessage(engine, {
        sessionPath,
        text: `[附件] ${filePath}`,
        displayMessage: {
          text: "",
          attachments: [{
            path: filePath,
            name: "voice.wav",
            isDir: false,
            mimeType: "audio/wav",
          }],
        },
      });

      expect(engine.promptSession).toHaveBeenCalledWith(
        sessionPath,
        `${sessionFileMarker({
          fileId: "sf_audio_attachment",
          sessionPath,
          label: "voice.wav",
        })}\n[附件] ${filePath}`,
        undefined,
      );
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("registers display audio attachments and forwards native audio paths when audio bytes are present", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-display-native-audio-"));
    try {
      const filePath = path.join(tmpDir, "voice.wav");
      fs.writeFileSync(filePath, Buffer.from([0x52, 0x49, 0x46, 0x46]));
      const session = makeFakeSession();
      const sessionPath = path.join(tmpDir, "main.jsonl");
      fs.writeFileSync(sessionPath, "{}\n");
      const registerSessionFile = vi.fn(({ sessionPath, filePath, label, origin, storageKind }) => ({
        id: "sf_audio_attachment",
        fileId: "sf_audio_attachment",
        sessionPath,
        filePath,
        realPath: filePath,
        displayName: label,
        filename: path.basename(filePath),
        label,
        ext: "wav",
        mime: "audio/wav",
        size: 4,
        kind: "audio",
        origin,
        storageKind,
        createdAt: 1,
      }));
      const queueVoiceTranscription = vi.fn();
      const engine = {
        lingxiHome: tmpDir,
        registerSessionFile,
        speechRecognition: { queueVoiceTranscription },
        ensureSessionLoaded: vi.fn(async () => session),
        promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
        emitEvent: vi.fn(),
        setUiContext: vi.fn(),
      };

      await submitDesktopSessionMessage(engine, {
        sessionPath,
        text: "hear this",
        audios: [{ type: "audio", data: "BASE64", mimeType: "audio/wav" }],
        displayMessage: {
          text: "hear this",
          attachments: [{
            path: filePath,
            name: "voice.wav",
            isDir: false,
            mimeType: "audio/wav",
          }],
        },
      });

      expect(registerSessionFile).toHaveBeenCalledWith({
        sessionPath,
        filePath,
        label: "voice.wav",
        origin: "user_attachment",
        storageKind: "external",
        presentation: "attachment",
        listed: true,
      });
      expect(engine.promptSession).toHaveBeenCalledWith(
        sessionPath,
        `${sessionFileMarker({
          fileId: "sf_audio_attachment",
          sessionPath,
          label: "voice.wav",
        })}\n[attached_audio: ${filePath}]\nhear this`,
        {
          audios: [{ type: "audio", data: "BASE64", mimeType: "audio/wav" }],
          audioAttachmentPaths: [filePath],
        },
      );
      expect(queueVoiceTranscription).not.toHaveBeenCalled();
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("queues transcription only for voice-input audio attachments with registered file ids", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-display-voice-input-"));
    try {
      const voicePath = path.join(tmpDir, "voice.wav");
      fs.writeFileSync(voicePath, Buffer.from([0x52, 0x49, 0x46, 0x46]));
      const session = makeFakeSession();
      const sessionPath = path.join(tmpDir, "main.jsonl");
      fs.writeFileSync(sessionPath, "{}\n");
      const registerSessionFile = vi.fn(({ sessionPath, filePath, label, origin, storageKind, presentation, listed }) => ({
        id: "sf_voice_input",
        fileId: "sf_voice_input",
        sessionPath,
        filePath,
        realPath: filePath,
        displayName: label,
        filename: path.basename(filePath),
        label,
        ext: "wav",
        mime: "audio/wav",
        size: 4,
        kind: "audio",
        origin,
        storageKind,
        presentation,
        listed,
        createdAt: 1,
      }));
      const queueVoiceTranscription = vi.fn();
      const engine = {
        lingxiHome: tmpDir,
        registerSessionFile,
        speechRecognition: { queueVoiceTranscription },
        ensureSessionLoaded: vi.fn(async () => session),
        promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
        emitEvent: vi.fn(),
        setUiContext: vi.fn(),
      };

      await submitDesktopSessionMessage(engine, {
        sessionPath,
        text: "",
        audios: [{ type: "audio", data: "BASE64", mimeType: "audio/wav" }],
        displayMessage: {
          text: "",
          attachments: [{
            path: voicePath,
            name: "录音 1.wav",
            isDir: false,
            mimeType: "audio/wav",
            presentation: "voice-input",
          }],
        },
      });

      expect(registerSessionFile).toHaveBeenCalledWith({
        sessionPath,
        filePath: voicePath,
        label: "录音 1.wav",
        origin: "voice_input",
        storageKind: "external",
        presentation: "voice-input",
        listed: false,
      });
      expect(queueVoiceTranscription).toHaveBeenCalledTimes(1);
      expect(queueVoiceTranscription).toHaveBeenCalledWith({
        sessionPath,
        fileId: "sf_voice_input",
      });
      expect(engine.promptSession).toHaveBeenCalledWith(
        sessionPath,
        `${sessionFileMarker({
          fileId: "sf_voice_input",
          sessionPath,
          label: "录音 1.wav",
        })}\n[attached_audio: ${voicePath}]`,
        {
          audios: [{ type: "audio", data: "BASE64", mimeType: "audio/wav" }],
          audioAttachmentPaths: [voicePath],
        },
      );
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("registers desktop display attachments into the session file ledger", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-display-attachment-"));
    try {
      const filePath = path.join(tmpDir, "desk.png");
      fs.writeFileSync(filePath, Buffer.from([0x89, 0x50, 0x4e, 0x47]));
      const session = makeFakeSession();
      const sessionPath = path.join(tmpDir, "main.jsonl");
      fs.writeFileSync(sessionPath, "{}\n");
      const registerSessionFile = vi.fn(({ sessionPath, filePath, label, origin, storageKind }) => ({
        id: "sf_desktop_attachment",
        fileId: "sf_desktop_attachment",
        sessionPath,
        filePath,
        realPath: filePath,
        displayName: label,
        filename: path.basename(filePath),
        label,
        ext: "png",
        mime: "image/png",
        size: 4,
        kind: "image",
        origin,
        storageKind,
        createdAt: 1,
      }));
      const engine = {
        lingxiHome: tmpDir,
        registerSessionFile,
        ensureSessionLoaded: vi.fn(async () => session),
        promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
        emitEvent: vi.fn(),
        setUiContext: vi.fn(),
      };

      await submitDesktopSessionMessage(engine, {
        sessionPath,
        text: "local file",
        images: [{ type: "image", data: "BASE64", mimeType: "image/png" }],
        displayMessage: {
          text: "local file",
          attachments: [{
            path: filePath,
            name: "desk.png",
            isDir: false,
            base64Data: "BASE64",
            mimeType: "image/png",
          }],
        },
      });

      expect(registerSessionFile).toHaveBeenCalledWith({
        sessionPath,
        filePath,
        label: "desk.png",
        origin: "user_attachment",
        storageKind: "external",
        presentation: "attachment",
        listed: true,
      });
      expect(engine.emitEvent).toHaveBeenCalledWith(
        expect.objectContaining({
          type: "session_user_message",
          message: expect.objectContaining({
            attachments: [expect.objectContaining({
              fileId: "sf_desktop_attachment",
              path: filePath,
            })],
          }),
        }),
        sessionPath,
      );
      const emittedAttachment = engine.emitEvent.mock.calls
        .find(([event]) => event.type === "session_user_message")?.[0].message.attachments[0];
      expect(emittedAttachment).not.toHaveProperty("base64Data");
      expect(engine.promptSession).toHaveBeenCalledWith(
        sessionPath,
        `${sessionFileMarker({
          fileId: "sf_desktop_attachment",
          sessionPath,
          label: "desk.png",
        })}\n[attached_image: ${filePath}]\nlocal file`,
        expect.objectContaining({
          imageAttachmentPaths: [filePath],
        }),
      );
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("registers bridge inbound files for desktop /rc target sessions", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-desktop-inbound-"));
    try {
      const session = makeFakeSession();
      const sessionPath = path.join(tmpDir, "agents", "hana", "sessions", "main.jsonl");
      fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
      fs.writeFileSync(sessionPath, "{}\n");
      const registerSessionFile = vi.fn(({ sessionPath, filePath, label, origin, storageKind }) => ({
        id: "sf_rc_inbound",
        fileId: "sf_rc_inbound",
        sessionPath,
        filePath,
        realPath: filePath,
        displayName: label,
        filename: path.basename(filePath),
        label,
        ext: "png",
        mime: "image/png",
        size: 4,
        kind: "image",
        origin,
        storageKind,
        createdAt: 1,
      }));
      const engine = {
        lingxiHome: tmpDir,
        registerSessionFile,
        ensureSessionLoaded: vi.fn(async () => session),
        promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
        emitEvent: vi.fn(),
        setUiContext: vi.fn(),
      };

      await submitDesktopSessionMessage(engine, {
        sessionPath,
        text: "see bridge image",
        images: [{ type: "image", data: "BASE64", mimeType: "image/png" }],
        inboundFiles: [{
          type: "image",
          filename: "bridge.png",
          mimeType: "image/png",
          buffer: Buffer.from([0x89, 0x50, 0x4e, 0x47]),
        }],
        displayMessage: { text: "see bridge image" },
      });

      const savedPath = registerSessionFile.mock.calls[0][0].filePath;
      expect(registerSessionFile).toHaveBeenCalledWith({
        sessionPath,
        filePath: expect.stringContaining(path.join(tmpDir, "session-files")),
        label: "bridge.png",
        origin: "bridge_inbound",
        storageKind: "managed_cache",
      });
      expect(engine.emitEvent).toHaveBeenCalledWith(
        expect.objectContaining({
          type: "session_user_message",
          message: expect.objectContaining({
            attachments: [expect.objectContaining({ fileId: "sf_rc_inbound", path: savedPath })],
          }),
        }),
        sessionPath,
      );
      expect(engine.promptSession).toHaveBeenCalledWith(
        sessionPath,
        `${sessionFileMarker({
          fileId: "sf_rc_inbound",
          sessionPath,
          label: "bridge.png",
        })}\n[attached_image: ${savedPath}]\nsee bridge image`,
        expect.objectContaining({
          imageAttachmentPaths: [savedPath],
        }),
      );
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("interjects into a streaming session after registering the same visible attachment envelope", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-desktop-interject-"));
    try {
      const filePath = path.join(tmpDir, "note.txt");
      fs.writeFileSync(filePath, "note");
      const sessionPath = path.join(tmpDir, "main.jsonl");
      fs.writeFileSync(sessionPath, "{}\n");
      const registerSessionFile = vi.fn(({ sessionPath, filePath, label, origin, storageKind }) => ({
        id: "sf_note",
        fileId: "sf_note",
        sessionPath,
        filePath,
        realPath: filePath,
        displayName: label,
        filename: path.basename(filePath),
        label,
        ext: "txt",
        mime: "text/plain",
        size: 4,
        kind: "attachment",
        origin,
        storageKind,
        createdAt: 1,
      }));
      const engine = {
        lingxiHome: tmpDir,
        registerSessionFile,
        ensureSessionLoaded: vi.fn(async () => makeFakeSession()),
        isSessionStreaming: vi.fn(() => true),
        promptSession: vi.fn(),
        steerSession: vi.fn(() => true),
        emitEvent: vi.fn(),
        setUiContext: vi.fn(),
      };

      const result = await submitDesktopSessionInterjection(engine, {
        sessionPath,
        text: "[附件] note.txt",
        displayMessage: {
          text: "",
          attachments: [{
            path: filePath,
            name: "note.txt",
            isDir: false,
          }],
        },
        sessionFileRefs: [{
          fileId: "sf_note",
          sessionPath,
          label: "note.txt",
          kind: "attachment",
        }],
        uiContext: { currentTab: "chat" },
      });

      expect(result).toEqual({ text: null, toolMedia: [], steered: true });
      expect(engine.ensureSessionLoaded).toHaveBeenCalledWith(sessionPath);
      expect(engine.setUiContext).toHaveBeenCalledWith(sessionPath, { currentTab: "chat" });
      expect(engine.emitEvent).toHaveBeenCalledWith(
        expect.objectContaining({
          type: "session_user_message",
          message: expect.objectContaining({
            text: "",
            attachments: [expect.objectContaining({
              fileId: "sf_note",
              path: filePath,
              name: "note.txt",
            })],
          }),
        }),
        sessionPath,
      );
      expect(engine.steerSession).toHaveBeenCalledWith(
        sessionPath,
        `${sessionFileMarker({
          fileId: "sf_note",
          sessionPath,
          label: "note.txt",
        })}\n[附件] note.txt`,
      );
      expect(engine.promptSession).not.toHaveBeenCalled();
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("falls back to a normal prompt when an interject arrives after streaming already ended", async () => {
    const session = makeFakeSession({ replyText: "finished reply" });
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      isSessionStreaming: vi.fn(() => false),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      steerSession: vi.fn(() => true),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    const result = await submitDesktopSessionInterjection(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "late interject",
      displayMessage: { text: "late interject" },
    });

    expect(result).toMatchObject({ text: "finished reply" });
    expect(engine.promptSession).toHaveBeenCalledWith("/tmp/desk.jsonl", "late interject", undefined);
    expect(engine.steerSession).not.toHaveBeenCalled();
  });

  // #1610 孤儿写入修复：steer 被拒绝时不写 origin 条目
  it("does not write a message-origin entry when steerSession returns false (session_busy race)", async () => {
    const session = makeFakeSession();
    const appendCustomEntry = vi.fn();
    (session as any).sessionManager = { appendCustomEntry };
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      isSessionStreaming: vi.fn(() => true),
      steerSession: vi.fn(() => false),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    await expect(submitDesktopSessionInterjection(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "interject",
      displayMessage: {
        text: "interject",
        source: "bridge_rc",
        bridgeSessionKey: "telegram:99",
      },
    })).rejects.toThrow("session_busy");

    // origin 条目必须在 steer 成功后才写，被拒绝时不能产生孤儿
    expect(appendCustomEntry).not.toHaveBeenCalled();
  });
});

describe("session reminder block injection", () => {
  const reminderBlock = "[hana_reminder]\n- 当前时间：2026-07-05 14:05\n[/hana_reminder]";
  const receipt = Object.freeze({
    observedAt: 1783231500000,
    throughSeq: 7,
    compactionRevision: 3,
  });

  it("prepends reminders before attachment markers and consumes the exact receipt after prompt acceptance", async () => {
    const session = makeFakeSession();
    (session as any).sessionManager = { appendCustomEntry: vi.fn() };
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
      renderSessionReminderBlock: vi.fn(() => ({ block: reminderBlock, receipt })),
      consumeRenderedSessionReminderBlock: vi.fn(),
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "hello",
      imageAttachmentPaths: ["/tmp/image.png"],
      displayMessage: { text: "hello" },
      context: { beforeUser: "world lore" },
    });

    expect(engine.promptSession).toHaveBeenCalledWith(
      "/tmp/desk.jsonl",
      `${reminderBlock}\n\n[attached_image: /tmp/image.png]\nhello`,
      { imageAttachmentPaths: ["/tmp/image.png"], context: { beforeUser: "world lore" } },
    );
    expect(engine.consumeRenderedSessionReminderBlock).toHaveBeenCalledWith("/tmp/desk.jsonl", receipt);
    expect((session as any).sessionManager.appendCustomEntry).toHaveBeenCalledWith(
      MESSAGE_PRESENTATION_RECORD_TYPE,
      expect.objectContaining({ displayText: "hello" }),
    );
    expect(engine.emitEvent).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "session_user_message",
        message: expect.objectContaining({ text: "hello" }),
      }),
      "/tmp/desk.jsonl",
    );
  });

  it("does not consume a rendered receipt when promptSession rejects", async () => {
    const session = makeFakeSession();
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async () => { throw new Error("model preflight failed"); }),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
      renderSessionReminderBlock: vi.fn(() => ({ block: reminderBlock, receipt })),
      consumeRenderedSessionReminderBlock: vi.fn(),
    };

    await expect(submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "hello",
      displayMessage: { text: "hello" },
    })).rejects.toThrow("model preflight failed");

    expect(engine.consumeRenderedSessionReminderBlock).not.toHaveBeenCalled();
  });

  it("does not rerender through the legacy API when the render API reports no reminder", async () => {
    const session = makeFakeSession();
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
      renderSessionReminderBlock: vi.fn(() => null),
      consumeSessionReminderBlock: vi.fn(() => reminderBlock),
      consumeRenderedSessionReminderBlock: vi.fn(),
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "hello",
      displayMessage: { text: "hello" },
    });

    expect(engine.promptSession).toHaveBeenCalledWith("/tmp/desk.jsonl", "hello", undefined);
    expect(engine.consumeSessionReminderBlock).not.toHaveBeenCalled();
  });

  it("ignores destructive consume-only reminders while preserving numeric rendered receipts", async () => {
    const legacySession = makeFakeSession();
    const consumeOnlyEngine = {
      ensureSessionLoaded: vi.fn(async () => legacySession),
      promptSession: vi.fn(async (sessionPath, text, opts) => legacySession.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
      consumeSessionReminderBlock: vi.fn(() => reminderBlock),
    };
    await submitDesktopSessionMessage(consumeOnlyEngine, {
      sessionPath: "/tmp/legacy.jsonl",
      text: "hello",
      displayMessage: { text: "hello" },
    });
    expect(consumeOnlyEngine.promptSession).toHaveBeenCalledWith("/tmp/legacy.jsonl", "hello", undefined);
    expect(consumeOnlyEngine.consumeSessionReminderBlock).not.toHaveBeenCalled();

    const numericSession = makeFakeSession();
    const numericEngine = {
      ensureSessionLoaded: vi.fn(async () => numericSession),
      promptSession: vi.fn(async (sessionPath, text, opts) => numericSession.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
      renderSessionReminderBlock: vi.fn(() => ({ block: reminderBlock, now: 1783231500000 })),
      consumeRenderedSessionReminderBlock: vi.fn(),
    };
    await submitDesktopSessionMessage(numericEngine, {
      sessionPath: "/tmp/numeric.jsonl",
      text: "hello",
      displayMessage: { text: "hello" },
    });
    expect(numericEngine.consumeRenderedSessionReminderBlock)
      .toHaveBeenCalledWith("/tmp/numeric.jsonl", 1783231500000);
  });

  it("puts reminder, beforeUser context, attachment marker, and body in stable interjection order", async () => {
    const session = makeFakeSession();
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      isSessionStreaming: vi.fn(() => true),
      steerSession: vi.fn(() => true),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
      renderSessionReminderBlock: vi.fn(() => ({ block: reminderBlock, receipt })),
      consumeRenderedSessionReminderBlock: vi.fn(),
    };

    await submitDesktopSessionInterjection(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "interject now",
      imageAttachmentPaths: ["/tmp/image.png"],
      displayMessage: { text: "interject now" },
      context: { beforeUser: "world lore" },
    });

    expect(engine.steerSession).toHaveBeenCalledWith(
      "/tmp/desk.jsonl",
      `${reminderBlock}\n\nworld lore\n\n[attached_image: /tmp/image.png]\ninterject now`,
    );
    expect(engine.consumeRenderedSessionReminderBlock).toHaveBeenCalledWith("/tmp/desk.jsonl", receipt);
  });

  it("keeps a rendered receipt pending when steerSession rejects", async () => {
    const session = makeFakeSession();
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      isSessionStreaming: vi.fn(() => true),
      steerSession: vi.fn(() => false),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
      renderSessionReminderBlock: vi.fn(() => ({ block: reminderBlock, receipt })),
      consumeRenderedSessionReminderBlock: vi.fn(),
    };

    await expect(submitDesktopSessionInterjection(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "interject now",
      displayMessage: { text: "interject now" },
    })).rejects.toThrow("session_busy");

    expect(engine.consumeRenderedSessionReminderBlock).not.toHaveBeenCalled();
    expect(engine.emitEvent).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: "session_user_message" }),
      expect.anything(),
    );
  });

  it("publishes prompt side effects only inside the synchronous post-preflight hook", async () => {
    const session = makeFakeSession();
    (session as any).sessionManager = { appendCustomEntry: vi.fn() };
    const order: string[] = [];
    const engine = {
      preflightSessionInput: vi.fn(),
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (_sessionPath, text, opts, submitOptions) => {
        order.push("cache-preflight");
        expect((session as any).sessionManager.appendCustomEntry).not.toHaveBeenCalled();
        // 发送即置忙：进 preflight 时只允许已发过一次提前的 session_status(true)。
        expect(engine.emitEvent).toHaveBeenCalledTimes(1);
        expect(engine.emitEvent).toHaveBeenNthCalledWith(
          1,
          expect.objectContaining({ type: "session_status", isStreaming: true }),
          "/tmp/desk.jsonl",
        );
        const hookResult = submitOptions.afterCachePreflight();
        expect(hookResult).toBeUndefined();
        order.push("pi-prompt");
        await session.prompt(text, opts);
      }),
      emitEvent: vi.fn((event) => order.push(event.type)),
      setUiContext: vi.fn(),
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "hello",
      displayMessage: { text: "visible", source: "bridge_rc" },
      beforeInputSideEffects: () => { order.push("retry-branch-commit"); },
    });

    expect(order.slice(0, 6)).toEqual([
      "session_status",
      "cache-preflight",
      "retry-branch-commit",
      "session_status",
      "session_user_message",
      "pi-prompt",
    ]);
    expect((session as any).sessionManager.appendCustomEntry).toHaveBeenCalled();
  });

  it("leaves no prompt events, custom entries, or consumed receipt when preflight rejects", async () => {
    const session = makeFakeSession();
    const appendCustomEntry = vi.fn();
    (session as any).sessionManager = { appendCustomEntry };
    const engine = {
      preflightSessionInput: vi.fn(),
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async () => { throw new Error("Cache prefix contract violated: tools"); }),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
      renderSessionReminderBlock: vi.fn(() => ({ block: reminderBlock, receipt })),
      consumeRenderedSessionReminderBlock: vi.fn(),
    };
    const beforeInputSideEffects = vi.fn();

    await expect(submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "hello",
      displayMessage: { text: "hello", source: "bridge_rc" },
      beforeInputSideEffects,
    })).rejects.toThrow("Cache prefix contract violated");

    // 发送即置忙 + 失败回收：preflight 拒绝只留下这对 status（无用户投影等其它事件）。
    expect(engine.emitEvent).toHaveBeenCalledTimes(2);
    expect(engine.emitEvent).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({ type: "session_status", isStreaming: true }),
      "/tmp/desk.jsonl",
    );
    expect(engine.emitEvent).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({ type: "session_status", isStreaming: false }),
      "/tmp/desk.jsonl",
    );
    expect(appendCustomEntry).not.toHaveBeenCalled();
    expect(engine.consumeRenderedSessionReminderBlock).not.toHaveBeenCalled();
    expect(beforeInputSideEffects).not.toHaveBeenCalled();
  });

  it("acceptance receipt rejects for an immediate preflight failure", async () => {
    const session = makeFakeSession();
    const engine = {
      preflightSessionInput: vi.fn(),
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async () => { throw new Error("prompt preflight rejected"); }),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    const submission = submitDesktopSessionMessageWithReceipt(engine, {
      sessionPath: "/tmp/receipt-fast-reject.jsonl",
      text: "hello",
    });
    await expect(submission.accepted).rejects.toThrow("prompt preflight rejected");
    await expect(submission.completion).rejects.toThrow("prompt preflight rejected");
    // 发送即置忙 + 失败回收：只有这对 status，无用户投影。
    expect(engine.emitEvent).toHaveBeenCalledTimes(2);
    expect(engine.emitEvent).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({ type: "session_status", isStreaming: true }),
      "/tmp/receipt-fast-reject.jsonl",
    );
    expect(engine.emitEvent).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({ type: "session_status", isStreaming: false }),
      "/tmp/receipt-fast-reject.jsonl",
    );
  });

  it("acceptance receipt stays pending through delayed preflight and rejects when it finally fails", async () => {
    const session = makeFakeSession();
    let finishPreflight!: () => void;
    const preflightGate = new Promise<void>((resolve) => { finishPreflight = resolve; });
    const engine = {
      preflightSessionInput: vi.fn(),
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async () => {
        await preflightGate;
        throw new Error("delayed prompt preflight rejected");
      }),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    const submission = submitDesktopSessionMessageWithReceipt(engine, {
      sessionPath: "/tmp/receipt-delayed-reject.jsonl",
      text: "hello",
    });
    let acceptedSettled = false;
    void submission.accepted.finally(() => { acceptedSettled = true; }).catch(() => {});
    await Promise.resolve();
    expect(acceptedSettled).toBe(false);
    finishPreflight();
    await expect(submission.accepted).rejects.toThrow("delayed prompt preflight rejected");
    await expect(submission.completion).rejects.toThrow("delayed prompt preflight rejected");
    // 发送即置忙 + 失败回收：只有这对 status，无用户投影。
    expect(engine.emitEvent).toHaveBeenCalledTimes(2);
    expect(engine.emitEvent).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({ type: "session_status", isStreaming: true }),
      "/tmp/receipt-delayed-reject.jsonl",
    );
    expect(engine.emitEvent).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({ type: "session_status", isStreaming: false }),
      "/tmp/receipt-delayed-reject.jsonl",
    );
  });

  it("receipt resolves after accepted side effects without waiting for the model turn", async () => {
    const session = makeFakeSession();
    let finishTurn!: () => void;
    const turnGate = new Promise<void>((resolve) => { finishTurn = resolve; });
    const engine = {
      preflightSessionInput: vi.fn(),
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (_sessionPath, _text, _opts, submitOptions) => {
        submitOptions.afterCachePreflight();
        submitOptions.afterInputAccepted();
        await turnGate;
        throw new Error("provider failed after acceptance");
      }),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    const submission = submitDesktopSessionMessageWithReceipt(engine, {
      sessionPath: "/tmp/receipt-accepted.jsonl",
      text: "hello",
      displayMessage: { text: "hello" },
    });
    await expect(submission.accepted).resolves.toMatchObject({
      accepted: true,
      sessionPath: "/tmp/receipt-accepted.jsonl",
    });
    expect(engine.emitEvent).toHaveBeenCalledWith(
      expect.objectContaining({ type: "session_user_message" }),
      "/tmp/receipt-accepted.jsonl",
    );
    finishTurn();
    await expect(submission.completion).rejects.toThrow("provider failed after acceptance");
  });

  it("closes streaming status but retains the receipt when Pi prompt fails after the hook", async () => {
    const session = makeFakeSession();
    const engine = {
      preflightSessionInput: vi.fn(),
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (_sessionPath, _text, _opts, submitOptions) => {
        submitOptions.afterCachePreflight();
        throw new Error("provider rejected prompt");
      }),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
      renderSessionReminderBlock: vi.fn(() => ({ block: reminderBlock, receipt })),
      consumeRenderedSessionReminderBlock: vi.fn(),
    };

    await expect(submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "hello",
      displayMessage: { text: "hello" },
    })).rejects.toThrow("provider rejected prompt");

    expect(engine.emitEvent.mock.calls
      .filter(([event]) => event.type === "session_status")
      .map(([event]) => event.isStreaming)).toEqual([true, true, false]);
    expect(engine.consumeRenderedSessionReminderBlock).not.toHaveBeenCalled();
  });

  it("consumes a silent recovery receipt without changing prompt or presentation", async () => {
    const session = makeFakeSession();
    const appendCustomEntry = vi.fn();
    (session as any).sessionManager = { appendCustomEntry };
    const recoveryReceipt = { ...receipt, unavailableToolNames: [] };
    const engine = {
      preflightSessionInput: vi.fn(),
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (_sessionPath, text, opts, submitOptions) => {
        submitOptions.afterCachePreflight();
        await session.prompt(text, opts);
      }),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
      renderSessionReminderBlock: vi.fn(() => ({ block: "", receipt: recoveryReceipt })),
      consumeRenderedSessionReminderBlock: vi.fn(),
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "hello",
      displayMessage: { text: "hello" },
    });

    expect(engine.promptSession.mock.calls[0][1]).toBe("hello");
    expect(appendCustomEntry).not.toHaveBeenCalledWith(
      MESSAGE_PRESENTATION_RECORD_TYPE,
      expect.anything(),
    );
    expect(engine.consumeRenderedSessionReminderBlock)
      .toHaveBeenCalledWith("/tmp/desk.jsonl", recoveryReceipt);
  });

  it("keeps steer failures completely side-effect free when cache preflight throws", async () => {
    const session = makeFakeSession();
    const appendCustomEntry = vi.fn();
    (session as any).sessionManager = { appendCustomEntry };
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      isSessionStreaming: vi.fn(() => true),
      steerSession: vi.fn(() => { throw new Error("Cache prefix contract violated: tools"); }),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
      renderSessionReminderBlock: vi.fn(() => ({ block: reminderBlock, receipt })),
      consumeRenderedSessionReminderBlock: vi.fn(),
    };

    await expect(submitDesktopSessionInterjection(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "interject",
      displayMessage: { text: "interject", source: "bridge_rc" },
    })).rejects.toThrow("Cache prefix contract violated");

    expect(engine.emitEvent).not.toHaveBeenCalled();
    expect(appendCustomEntry).not.toHaveBeenCalled();
    expect(engine.consumeRenderedSessionReminderBlock).not.toHaveBeenCalled();
  });
});


describe("knowledgeRefs passthrough (Phase 7)", () => {
  it("projects displayMessage.knowledgeRefs into the user message event and presentation record", async () => {
    const session = makeFakeSession();
    const appendCustomEntry = vi.fn();
    Object.assign(session, { sessionManager: { appendCustomEntry } });
    const retrievalStats = {
      mode: "qa",
      retrievalMode: "fts",
      subQueries: ["总结一下"],
      subQueryHits: [2],
      degraded: true,
      degradeReason: "knowledge model slot not configured",
      fusedChunks: 2,
      injectedChunks: 2,
      truncated: false,
      usedTokens: 64,
      budgetTokens: 6000,
    };
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
      buildKnowledgeContextInjection: vi.fn(async () => ({
        block: "[KnowledgeContext]\nevidence\n[/KnowledgeContext]",
        stats: retrievalStats,
      })),
    };
    const knowledgeRefs = {
      notebookIds: ["nb-1", "nb-2"],
      mode: "qa",
      notebooks: [{ id: "nb-1", name: "产品笔记" }, { id: "nb-2", name: "小说资料" }],
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "总结一下",
      // 功能字段（服务端校验后透传）
      knowledgeRefs: { notebookIds: ["nb-1", "nb-2"], mode: "qa" },
      // 展示投影（含名称缓存）
      displayMessage: { text: "总结一下", knowledgeRefs },
    });

    expect(engine.emitEvent).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "session_user_message",
        message: expect.objectContaining({ knowledgeRefs, knowledgeRetrieval: retrievalStats }),
      }),
      "/tmp/desk.jsonl",
    );
    expect(appendCustomEntry).toHaveBeenCalledWith(
      MESSAGE_PRESENTATION_RECORD_TYPE,
      expect.objectContaining({ displayText: "总结一下", knowledgeRefs, knowledgeRetrieval: retrievalStats }),
    );
    // Phase 8：注入块进入模型 prompt（见下方 Phase 8 专测），投影保持原文。
    const promptText = engine.promptSession.mock.calls[0][1];
    expect(promptText).toContain("总结一下");
  });

  it("does not write a presentation record when knowledgeRefs is absent", async () => {
    const session = makeFakeSession();
    const appendCustomEntry = vi.fn();
    Object.assign(session, { sessionManager: { appendCustomEntry } });
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "普通消息",
    });

    expect(appendCustomEntry).not.toHaveBeenCalled();
    const userMessageEvent = engine.emitEvent.mock.calls.find(
      ([event]) => event?.type === "session_user_message",
    );
    expect(userMessageEvent?.[0]?.message?.knowledgeRefs ?? null).toBeNull();
  });

  it("rejects malformed knowledgeRefs explicitly instead of dropping them silently", async () => {
    const session = makeFakeSession();
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    await expect(submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "你好",
      knowledgeRefs: { notebookIds: ["nb-1"], mode: "strict" } as any,
    })).rejects.toThrow('knowledgeRefs.mode must be "qa" or "assist"');

    await expect(submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "你好",
      knowledgeRefs: { notebookIds: [""], mode: "qa" } as any,
    })).rejects.toThrow("knowledgeRefs.notebookIds must be an array of non-empty strings");

    expect(engine.promptSession).not.toHaveBeenCalled();
  });

  it("rejects malformed knowledgeRefs on the interjection path too", async () => {
    const session = makeFakeSession();
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      isSessionStreaming: vi.fn(() => true),
      steerSession: vi.fn(() => true),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    await expect(submitDesktopSessionInterjection(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "插话",
      knowledgeRefs: { notebookIds: "nb-1", mode: "qa" } as any,
    })).rejects.toThrow("knowledgeRefs.notebookIds must be an array of non-empty strings");

    expect(engine.steerSession).not.toHaveBeenCalled();
  });

  it("normalizes duplicate/whitespace notebookIds and treats empty refs as absent", async () => {
    const session = makeFakeSession();
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
    };

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "你好",
      knowledgeRefs: { notebookIds: [], mode: "qa" },
    });
    // 空数组归一为无引用，正常提交
    expect(engine.promptSession).toHaveBeenCalledWith("/tmp/desk.jsonl", "你好", undefined);
  });
});

describe("knowledge context injection (Phase 8)", () => {
  const INJECTION_BLOCK = "[KnowledgeContext]\n[K1] evidence\n[/KnowledgeContext]";
  const RETRIEVAL_STATS = {
    mode: "qa",
    retrievalMode: "hybrid",
    subQueries: ["苹果 交付"],
    subQueryHits: [2],
    degraded: false,
    fusedChunks: 2,
    injectedChunks: 1,
    truncated: true,
    usedTokens: 5980,
    budgetTokens: 6000,
  };

  function injectionEngine(overrides: Record<string, any> = {}) {
    const session = makeFakeSession();
    const appendCustomEntry = vi.fn();
    Object.assign(session, { sessionManager: { appendCustomEntry } });
    const engine = {
      ensureSessionLoaded: vi.fn(async () => session),
      promptSession: vi.fn(async (sessionPath, text, opts) => session.prompt(text, opts)),
      emitEvent: vi.fn(),
      setUiContext: vi.fn(),
      buildKnowledgeContextInjection: vi.fn(async () => ({ block: INJECTION_BLOCK, stats: RETRIEVAL_STATS })),
      ...overrides,
    };
    return { engine, session, appendCustomEntry };
  }

  it("prepends the injection block to the model prompt but keeps it out of the visible projection", async () => {
    const { engine, appendCustomEntry } = injectionEngine();

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "苹果什么时候交付",
      displayMessage: { text: "苹果什么时候交付" },
      knowledgeRefs: { notebookIds: ["nb-1"], mode: "qa" },
    });

    expect(engine.buildKnowledgeContextInjection).toHaveBeenCalledWith({
      question: "苹果什么时候交付",
      knowledgeRefs: { notebookIds: ["nb-1"], mode: "qa" },
      // mock 会话无 model.contextWindow → 动态预算回退固定兜底值。
      budgetTokens: 6000,
      // 会话路径随行：蒸馏进度事件按 session 广播（knowledge_distill_progress）。
      sessionPath: "/tmp/desk.jsonl",
      // Phase 9 第二波：检索期取消信号（exhaustive coverage run 中止通道）。
      signal: expect.any(AbortSignal),
    });
    expect(engine.promptSession).toHaveBeenCalledWith(
      "/tmp/desk.jsonl",
      `${INJECTION_BLOCK}\n\n苹果什么时候交付`,
      undefined,
    );
    // 用户可见投影（事件与展示条目）不包含注入块，但携带检索统计。
    const userMessage = engine.emitEvent.mock.calls
      .find(([event]) => event?.type === "session_user_message")?.[0].message;
    expect(userMessage.text).toBe("苹果什么时候交付");
    expect(userMessage.text).not.toContain("[KnowledgeContext]");
    expect(userMessage.knowledgeRetrieval).toBe(RETRIEVAL_STATS);
    expect(appendCustomEntry).toHaveBeenCalledWith(
      MESSAGE_PRESENTATION_RECORD_TYPE,
      expect.objectContaining({ displayText: "苹果什么时候交付", knowledgeRetrieval: RETRIEVAL_STATS }),
    );
    const presentationArg = appendCustomEntry.mock.calls
      .find(([type]) => type === MESSAGE_PRESENTATION_RECORD_TYPE)?.[1];
    expect(JSON.stringify(presentationArg)).not.toContain("[KnowledgeContext]");
  });

  it("forces a displayText presentation entry even without structured displayMessage", async () => {
    const { engine, appendCustomEntry } = injectionEngine();

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "只问一句",
      knowledgeRefs: { notebookIds: ["nb-1"], mode: "assist" },
    });

    // displayMessage 缺省时也必须持久化展示正文，否则历史投影会显示注入后的 prompt。
    expect(appendCustomEntry).toHaveBeenCalledWith(
      MESSAGE_PRESENTATION_RECORD_TYPE,
      expect.objectContaining({ displayText: "只问一句" }),
    );
    expect(engine.promptSession.mock.calls[0][1]).toContain("[KnowledgeContext]");
  });

  it("keeps the chat flowing with an explicit unavailable annotation when the injector throws", async () => {
    const { engine } = injectionEngine({
      buildKnowledgeContextInjection: vi.fn(async () => {
        throw new Error("embedding provider down");
      }),
    });

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "继续问",
      displayMessage: { text: "继续问" },
      knowledgeRefs: { notebookIds: ["nb-1"], mode: "qa" },
    });

    const promptText = engine.promptSession.mock.calls[0][1];
    expect(promptText).toContain("[knowledge injection unavailable: embedding provider down]");
    expect(promptText).toContain("继续问");
    const userMessage = engine.emitEvent.mock.calls
      .find(([event]) => event?.type === "session_user_message")?.[0].message;
    expect(userMessage.text).toBe("继续问");
    // 降级路径的 stats 带 unavailableReason，其余字段置零/none（禁静默降级）。
    expect(userMessage.knowledgeRetrieval).toMatchObject({
      mode: "qa",
      retrievalMode: "none",
      subQueries: [],
      subQueryHits: [],
      degraded: false,
      fusedChunks: 0,
      injectedChunks: 0,
      truncated: false,
      usedTokens: 0,
      budgetTokens: 6000,
      unavailableReason: "embedding provider down",
    });
  });

  it("rejects explicitly when the engine lacks the injection facade while refs are present", async () => {
    const { engine } = injectionEngine();
    delete (engine as any).buildKnowledgeContextInjection;

    await expect(submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "你好",
      knowledgeRefs: { notebookIds: ["nb-1"], mode: "qa" },
    })).rejects.toThrow("knowledge injection unavailable");
    expect(engine.promptSession).not.toHaveBeenCalled();
  });

  it("does not call the injector when knowledgeRefs is absent or normalized away", async () => {
    const { engine } = injectionEngine();

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "普通消息",
      displayMessage: { text: "普通消息" },
    });
    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "空引用",
      displayMessage: { text: "空引用" },
      knowledgeRefs: { notebookIds: [], mode: "qa" },
    });

    expect(engine.buildKnowledgeContextInjection).not.toHaveBeenCalled();
    expect(engine.promptSession).toHaveBeenLastCalledWith("/tmp/desk.jsonl", "空引用", undefined);
  });

  it("skips injection when replaying a preserved prompt envelope", async () => {
    const { engine } = injectionEngine();

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: `${INJECTION_BLOCK}\n\nenvelope text`,
      knowledgeRefs: { notebookIds: ["nb-1"], mode: "qa" },
      preservePromptEnvelope: true,
    } as any);

    expect(engine.buildKnowledgeContextInjection).not.toHaveBeenCalled();
    expect(engine.promptSession).toHaveBeenCalledWith(
      "/tmp/desk.jsonl",
      `${INJECTION_BLOCK}\n\nenvelope text`,
      undefined,
    );
    // 重放不重新检索：新消息不携带检索统计（retry/fork 路径不复制旧 stats）。
    const userMessage = engine.emitEvent.mock.calls
      .find(([event]) => event?.type === "session_user_message")?.[0].message;
    expect(userMessage.knowledgeRetrieval).toBeNull();
  });

  it("emits knowledge_retrieval_started before the blocking injection resolves", async () => {
    const { engine } = injectionEngine();
    let resolveInjection!: (value: { block: string; stats: typeof RETRIEVAL_STATS }) => void;
    engine.buildKnowledgeContextInjection = vi.fn(() => new Promise((resolve) => {
      resolveInjection = resolve;
    }));
    const submitted = submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "苹果什么时候交付",
      displayMessage: { text: "苹果什么时候交付" },
      knowledgeRefs: { notebookIds: ["nb-1"], mode: "qa" },
    });

    await vi.waitFor(() => {
      expect(engine.emitEvent).toHaveBeenCalledWith(
        { type: "knowledge_retrieval_started", sessionPath: "/tmp/desk.jsonl" },
        "/tmp/desk.jsonl",
      );
    });
    // 注入未完成：promptSession 尚未被调用，事件先于用户投影（发送即置忙的
    // 提前 session_status 是唯一允许更早的事件）。
    expect(engine.promptSession).not.toHaveBeenCalled();

    resolveInjection({ block: INJECTION_BLOCK, stats: RETRIEVAL_STATS });
    await submitted;
    const finalTypes = engine.emitEvent.mock.calls.map(([event]) => event.type);
    expect(finalTypes.indexOf("knowledge_retrieval_started")).toBeLessThan(finalTypes.indexOf("session_user_message"));
  });

  it("检索期间 abort：跳过 promptSession 与用户投影，补发 isStreaming:false 收回提前忙态", async () => {
    const { engine, appendCustomEntry } = injectionEngine();
    let resolveInjection!: (value: { block: string; stats: typeof RETRIEVAL_STATS }) => void;
    engine.buildKnowledgeContextInjection = vi.fn(() => new Promise((resolve) => {
      resolveInjection = resolve;
    }));
    const submitted = submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "检索中被停止",
      displayMessage: { text: "检索中被停止" },
      knowledgeRefs: { notebookIds: ["nb-1"], mode: "qa" },
    });

    await vi.waitFor(() => {
      expect(engine.emitEvent).toHaveBeenCalledWith(
        { type: "knowledge_retrieval_started", sessionPath: "/tmp/desk.jsonl" },
        "/tmp/desk.jsonl",
      );
    });
    // 用户点停止：abort 路由标记 pending 提交（sessionId 与 path 两种键都命中）。
    expect(abortPendingDesktopSubmission(engine, { sessionPath: "/tmp/desk.jsonl" })).toBe(true);

    resolveInjection({ block: INJECTION_BLOCK, stats: RETRIEVAL_STATS });
    const result = await submitted;
    expect(result).toEqual({ text: null, toolMedia: [] });
    expect(engine.promptSession).not.toHaveBeenCalled();
    expect(appendCustomEntry).not.toHaveBeenCalled();
    const types = engine.emitEvent.mock.calls.map(([event]) => event.type);
    expect(types).not.toContain("session_user_message");
    // 提前置忙 + abort 收回：最后一次 session_status 是 isStreaming:false 且带 aborted。
    const statusCalls = engine.emitEvent.mock.calls.filter(([event]) => event.type === "session_status");
    expect(statusCalls[statusCalls.length - 1][0]).toMatchObject({ isStreaming: false, aborted: true, reason: "user_abort" });
    // abort 消费后不留残留：同一 session 立即可再次提交。
    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "下一条",
      displayMessage: { text: "下一条" },
    });
    expect(engine.promptSession).toHaveBeenCalledTimes(1);
  });

  it("检索期间 abort 同步中止传给注入链的 AbortSignal（exhaustive coverage run 取消通道）", async () => {
    const { engine } = injectionEngine();
    let capturedSignal: AbortSignal | undefined;
    let rejectInjection!: (reason: Error) => void;
    (engine as any).buildKnowledgeContextInjection = vi.fn((input: any) => {
      capturedSignal = input?.signal;
      return new Promise((_resolve, reject) => {
        rejectInjection = reject;
      });
    });
    const submitted = submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "全文梳理",
      displayMessage: { text: "全文梳理" },
      knowledgeRefs: { notebookIds: ["nb-1"], mode: "qa" },
    });

    await vi.waitFor(() => {
      expect(capturedSignal).toBeInstanceOf(AbortSignal);
    });
    expect(capturedSignal!.aborted).toBe(false);
    // 用户点停止：controller 立即 abort（不再等注入完成后才被丢弃）。
    expect(abortPendingDesktopSubmission(engine, { sessionPath: "/tmp/desk.jsonl" })).toBe(true);
    expect(capturedSignal!.aborted).toBe(true);

    // 注入链随后以取消收场（此处模拟 engine 内部对 signal 的响应后返回）。
    rejectInjection(new Error("coverage run aborted"));
    const result = await submitted;
    expect(result).toEqual({ text: null, toolMedia: [] });
    expect(engine.promptSession).not.toHaveBeenCalled();
    // controller 注册表已清理：同 session 再提交拿到的是新的未中止 signal。
    (engine as any).buildKnowledgeContextInjection = vi.fn(async () => ({ block: null, stats: RETRIEVAL_STATS }));
    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "下一条",
      displayMessage: { text: "下一条" },
      knowledgeRefs: { notebookIds: ["nb-1"], mode: "qa" },
    });
    const secondCall = (engine.buildKnowledgeContextInjection as unknown as ReturnType<typeof vi.fn>).mock.calls[0][0];
    expect(secondCall.signal.aborted).toBe(false);
    expect(secondCall.signal).not.toBe(capturedSignal);
  });

  it("does not emit knowledge_retrieval_started when knowledgeRefs is absent", async () => {
    const { engine } = injectionEngine();

    await submitDesktopSessionMessage(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "普通消息",
      displayMessage: { text: "普通消息" },
    });

    const emittedTypes = engine.emitEvent.mock.calls.map(([event]) => event.type);
    expect(emittedTypes).not.toContain("knowledge_retrieval_started");
  });

  it("emits knowledge_retrieval_started on the interjection path before steering", async () => {
    const { engine } = injectionEngine();
    let resolveInjection!: (value: { block: string; stats: typeof RETRIEVAL_STATS }) => void;
    engine.buildKnowledgeContextInjection = vi.fn(() => new Promise((resolve) => {
      resolveInjection = resolve;
    }));
    const steerSession = vi.fn(() => true);
    Object.assign(engine, {
      isSessionStreaming: vi.fn(() => true),
      steerSession,
    });
    const submitted = submitDesktopSessionInterjection(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "插一句",
      displayMessage: { text: "插一句" },
      knowledgeRefs: { notebookIds: ["nb-1"], mode: "qa" },
    });

    await vi.waitFor(() => {
      expect(engine.emitEvent).toHaveBeenCalledWith(
        { type: "knowledge_retrieval_started", sessionPath: "/tmp/desk.jsonl" },
        "/tmp/desk.jsonl",
      );
    });
    expect(steerSession).not.toHaveBeenCalled();

    resolveInjection({ block: INJECTION_BLOCK, stats: RETRIEVAL_STATS });
    await submitted;
    expect(steerSession).toHaveBeenCalledWith("/tmp/desk.jsonl", `${INJECTION_BLOCK}\n\n插一句`);
  });

  it("injects into the interjection (steer) prompt as well without polluting the projection", async () => {
    const { engine, appendCustomEntry } = injectionEngine();
    const steerSession = vi.fn(() => true);
    Object.assign(engine, {
      isSessionStreaming: vi.fn(() => true),
      steerSession,
    });

    await submitDesktopSessionInterjection(engine, {
      sessionPath: "/tmp/desk.jsonl",
      text: "插一句",
      displayMessage: { text: "插一句" },
      knowledgeRefs: { notebookIds: ["nb-1"], mode: "qa" },
    });

    expect(steerSession).toHaveBeenCalledWith(
      "/tmp/desk.jsonl",
      `${INJECTION_BLOCK}\n\n插一句`,
    );
    const userMessage = engine.emitEvent.mock.calls
      .find(([event]) => event?.type === "session_user_message")?.[0].message;
    expect(userMessage.text).toBe("插一句");
    expect(userMessage.knowledgeRetrieval).toBe(RETRIEVAL_STATS);
    expect(appendCustomEntry).toHaveBeenCalledWith(
      MESSAGE_PRESENTATION_RECORD_TYPE,
      expect.objectContaining({ displayText: "插一句", knowledgeRetrieval: RETRIEVAL_STATS }),
    );
  });
});
