// P01-A06 负例：把 mt_ trace 身份当作 mc_ call 身份使用。
// 本文件是 strict 门禁的故意违规样本：品牌类型必须阻止跨身份误用
// （TS2322/TS2345 指向 ModelCallId）。被 tsconfig.test.json 显式排除，
// 永不进入常规工程；由 tests/core-contracts-strict.test.ts 显式注入。
import { mintModelTraceId } from "../../../lib/llm/model-call-identity.ts";
import type { ModelCallId } from "../../../shared/identity-brands.ts";

declare function recordModelCall(callId: ModelCallId): void;

export const negativeMisuse = () => recordModelCall(mintModelTraceId());
