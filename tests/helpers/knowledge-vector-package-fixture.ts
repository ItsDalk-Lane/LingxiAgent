import fs from "node:fs";
import path from "node:path";
import { requiredKnowledgeVectorRuntimeFiles } from "../../scripts/build-server-runtime-assets.mjs";

/** 仅用于归档结构单测；真实原生加载由各平台包内烟测验证。 */
export function writeKnowledgeVectorPackageFixture(serverDir: string) {
  for (const [platform, arch] of [["darwin", "arm64"], ["darwin", "x64"], ["win32", "x64"], ["linux", "x64"]]) {
    for (const relative of requiredKnowledgeVectorRuntimeFiles(platform, arch)) {
      const file = path.join(serverDir, relative);
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(file, relative.endsWith("package.json") ? JSON.stringify({ version: "2.26.0" }) : "structure fixture");
    }
  }
}
