export interface MapTurn {
  turnIndex: number;
  questionEntryId: string | null;
  question: string;
  questionAt: string | null;
  answerEntryId: string | null;
  answer: string;
  answerAt: string | null;
  processCount: number;
  entryIds: string[];
  truncated: boolean;
  error?: string | null;
}

export interface MapTurnsEntry {
  revision: string | null;
  turns: MapTurn[];
  loading: boolean;
  error: string | null;
}

export interface MapCamera {
  x: number;
  y: number;
  zoom: number;
}

export type MapCardPosition = { x: number; y: number };

export interface MapDraft {
  kind: 'continue' | 'branch';
  anchorCardId: string;
  sessionId: string;
  sessionPath: string;
  agentId: string | null;
  text: string;
  sending: boolean;
}

export interface MapPendingTurn {
  question: string;
  startedAt: number;
}

export interface ConversationMapSlice {
  mapActiveWorkspaceKey: string | null;
  mapTurnsBySessionId: Record<string, MapTurnsEntry>;
  mapCameraByWorkspace: Record<string, MapCamera>;
  mapSelectedCardId: string | null;
  mapCardPositions: Record<string, MapCardPosition>;
  mapCollapsedCardIds: string[];
  /** 布局已从服务端拉取过（避免重复 GET 覆盖本地新状态）。 */
  mapLayoutLoaded: boolean;
  mapDraft: MapDraft | null;
  /** 已提交传输、尚未落盘的轮次（按 sessionPath 索引），用于渲染待回复卡片。 */
  mapPendingTurns: Record<string, MapPendingTurn>;
  setMapActiveWorkspace: (key: string | null) => void;
  setMapCamera: (workspaceKey: string, camera: MapCamera) => void;
  clearMapCamera: (workspaceKey: string) => void;
  setMapSelectedCard: (id: string | null) => void;
  setMapTurnsEntry: (sessionId: string, entry: MapTurnsEntry) => void;
  setMapCardPositions: (positions: Record<string, MapCardPosition>) => void;
  mergeMapCardPosition: (cardId: string, position: MapCardPosition) => void;
  setMapCollapsedCardIds: (ids: string[]) => void;
  setMapLayoutLoaded: (loaded: boolean) => void;
  setMapDraft: (draft: MapDraft | null) => void;
  updateMapDraftText: (text: string) => void;
  setMapDraftSending: (sending: boolean) => void;
  setMapPendingTurn: (sessionPath: string, entry: MapPendingTurn) => void;
  clearMapPendingTurn: (sessionPath: string) => void;
}

export const createConversationMapSlice = (
  set: (partial: Partial<ConversationMapSlice> | ((s: ConversationMapSlice) => Partial<ConversationMapSlice>)) => void,
): ConversationMapSlice => ({
  mapActiveWorkspaceKey: null,
  mapTurnsBySessionId: {},
  mapCameraByWorkspace: {},
  mapSelectedCardId: null,
  mapCardPositions: {},
  mapCollapsedCardIds: [],
  mapLayoutLoaded: false,
  mapDraft: null,
  mapPendingTurns: {},
  setMapActiveWorkspace: (key) => set({ mapActiveWorkspaceKey: key }),
  setMapCamera: (workspaceKey, camera) => set((s) => ({
    mapCameraByWorkspace: { ...s.mapCameraByWorkspace, [workspaceKey]: camera },
  })),
  clearMapCamera: (workspaceKey) => set((s) => {
    const next = { ...s.mapCameraByWorkspace };
    delete next[workspaceKey];
    return { mapCameraByWorkspace: next };
  }),
  setMapSelectedCard: (id) => set({ mapSelectedCardId: id }),
  setMapTurnsEntry: (sessionId, entry) => set((s) => ({
    mapTurnsBySessionId: { ...s.mapTurnsBySessionId, [sessionId]: entry },
  })),
  setMapCardPositions: (positions) => set({ mapCardPositions: positions }),
  mergeMapCardPosition: (cardId, position) => set((s) => ({
    mapCardPositions: { ...s.mapCardPositions, [cardId]: position },
  })),
  setMapCollapsedCardIds: (ids) => set({ mapCollapsedCardIds: ids }),
  setMapLayoutLoaded: (loaded) => set({ mapLayoutLoaded: loaded }),
  setMapDraft: (draft) => set({ mapDraft: draft }),
  updateMapDraftText: (text) => set((s) => (
    s.mapDraft ? { mapDraft: { ...s.mapDraft, text } } : {}
  )),
  setMapDraftSending: (sending) => set((s) => (
    s.mapDraft ? { mapDraft: { ...s.mapDraft, sending } } : {}
  )),
  setMapPendingTurn: (sessionPath, entry) => set((s) => ({
    mapPendingTurns: { ...s.mapPendingTurns, [sessionPath]: entry },
  })),
  clearMapPendingTurn: (sessionPath) => set((s) => {
    if (!Object.prototype.hasOwnProperty.call(s.mapPendingTurns, sessionPath)) return {};
    const next = { ...s.mapPendingTurns };
    delete next[sessionPath];
    return { mapPendingTurns: next };
  }),
});
