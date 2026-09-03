import type { LocalModelCategory, LocalModelTier } from "./contracts.ts";

export interface LocalModelCatalogVariant {
  quant: string;
  tier: LocalModelTier;
  estimatedPeakRssMb: number | null;
  default: boolean;
}

export interface LocalModelCatalogEntry {
  id: string;
  category: LocalModelCategory;
  displayName: string;
  runtimeId: string;
  license: string;
  variants: readonly LocalModelCatalogVariant[];
  distributionStatus: "catalog-only" | "manifest-available";
}

/**
 * 产品目录只冻结用户要求的稳定身份，不伪造可下载 URL、大小或哈希。
 * 只有远端 manifest 通过严格校验后，条目才会被注册表提升为可下载。
 */
export const BUILTIN_LOCAL_MODEL_CATALOG: readonly LocalModelCatalogEntry[] = Object.freeze([
  entry("qwen3-embedding-8b", "embedding", "Qwen3 Embedding 8B", "llama.cpp", "Apache-2.0", [
    variant("q4_k_m", "large", 9000, true),
  ]),
  entry("glm-ocr", "ocr", "GLM-OCR", "llama.cpp", "Apache-2.0", [
    variant("bf16", "large", 8000, true),
  ]),
  entry("qwen3-asr-1.7b", "stt", "Qwen3 ASR 1.7B", "llama.cpp", "Apache-2.0", [
    variant("bf16", "large", 5800, true),
  ]),
  entry("indextts-2.5", "tts", "IndexTTS 2.5", "audio.cpp", "bilibili-model-license", [
    variant("q8_0", "large", 6500, true),
  ]),
]);

function entry(
  id: string,
  category: LocalModelCategory,
  displayName: string,
  runtimeId: string,
  license: string,
  variants: LocalModelCatalogVariant[],
): LocalModelCatalogEntry {
  return Object.freeze({
    id,
    category,
    displayName,
    runtimeId,
    license,
    variants: Object.freeze(variants),
    distributionStatus: "catalog-only",
  });
}

function variant(
  quant: string,
  tier: LocalModelTier,
  estimatedPeakRssMb: number | null,
  isDefault = false,
): LocalModelCatalogVariant {
  return Object.freeze({ quant, tier, estimatedPeakRssMb, default: isDefault });
}

