# R01-T05 样本字体来源与许可

样本 HTML 的字体栈与现役生产契约一致：`desktop/src/office-pdf-fonts.cjs` 的
`LINGXI_PDF_FONT_FAMILIES = ["EB Garamond", "Noto Serif SC", "JetBrains Mono"]`，
衬线栈 `'EB Garamond', 'Noto Serif SC', serif`，等宽栈 `'JetBrains Mono', monospace`。

测试使用的字体文件即仓库内随产品分发的同一批 woff2（`desktop/src/themes/fonts/`），
不引入任何第三方测试字体：

| 字体族 | 文件 | 上游项目 | 许可 | 许可文本核验 |
|---|---|---|---|---|
| EB Garamond | `ebgaramond-*.woff2`（4 文件） | google/fonts `ofl/ebgaramond`（Octavio Pardo 等） | SIL Open Font License 1.1 | 见下 |
| Noto Serif SC | `notoserifsc-*.woff2`（约 100 个 unicode-range 子集） | google/fonts `ofl/notoserifsc`（Adobe + Google） | SIL Open Font License 1.1 | 见下 |
| JetBrains Mono | `jetbrainsmono-*.woff2`（2 文件） | google/fonts `ofl/jetbrainsmono`（JetBrains） | SIL Open Font License 1.1 | 见下 |

核验方式（本机离线）：
1. `desktop/src/themes/new-warm-paper-fonts.css` 的 `@font-face` 块即上述三族
   （另有 Inter/PT Serif 供 UI 使用，不在 PDF 注入白名单内）；
2. 三族均为 Google Fonts 收录字体，Google Fonts 全部字体的许可页
   （https://fonts.google.com 对应字体页 "License" 栏）标注 SIL OFL 1.1；
   本任务不联网复核（测试红线），许可以仓库内既有分发事实 + 上游公开许可登记为准，
   字体文件哈希见 `artifacts/rust-tauri/R01/T05/SHA256SUMS.txt` 样本节；
3. OFL 1.1 允许随产品再分发与子集化嵌入，保留许可声明的义务由产品打包流程承担
   （现状：仓库随包分发，未在本任务新增义务）。

子集覆盖实测（`generate_samples.py` 同源脚本对 CSS unicode-range 的解析结果）：

| 字符 | 码位 | Noto Serif SC 子集覆盖 |
|---|---|---|
| 龘 | U+9F98 | 是 |
| 靐 | U+9750 | 是 |
| 麤 | U+9EA4 | 是 |
| 爨 | U+7228 | 是 |
| 驫 | U+9A6B | 是 |
| 鬱 | U+9B31 | 是 |
| 齉 | U+9F49 | 否（回退系统字体，两条链行为一致，见 PDF_SPIKE_REPORT §4） |
| 爩 | U+7229 | 否（同上） |
| 龗 | U+9F97 | 否（同上） |
| 灪 | U+706A | 否（同上） |
| 𠮷 | U+20BB7（Ext-B） | 否（同上） |
| 𪚥 | U+2A6A5（Ext-B） | 否（同上） |

样本故意同时包含子集内与子集外生僻字：前者验证注入字体真实参与生僻字渲染，
后者验证回退路径不产生豆腐块（两链对照 + 墨迹覆盖判据）。
