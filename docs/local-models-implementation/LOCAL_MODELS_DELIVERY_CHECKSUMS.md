# 本地模型交付校验和

日期：2026-09-02  
算法：SHA-256

本文件是源码包生成后的独立校验清单，因此不包含在源码 ZIP 内，也不记录自身摘要。

| 文件 | 字节数 | SHA-256 |
|---|---:|---|
| `/Users/study_superior/Downloads/LingxiAgent-local-models-codex-3eab8589-20260902.zip` | 52,618,906 | `a3b69a6efd5c92e44728265759ed720ee86c6d76877b75d579d7a50bbde5accb` |
| `LOCAL_MODELS_IMPLEMENTATION_REPORT.md` | 5,539 | `c437546c33511a2a4e0b5b9613515a32b7b248fea4f6132957be532df99e445d` |
| `LOCAL_MODELS_TEST_REPORT.md` | 4,269 | `275634ac6d6d0a91ac43207174f5d7d3f3a2ca24d345d089352b0fef14563958` |
| `LOCAL_MODELS_SOURCE_AUDIT.md` | 3,947 | `0f4cb57a745d1f27325d6ab13a10871125f4368347cd5381f7738d46b553b3e1` |
| `LOCAL_MODELS_REMAINING.md` | 3,645 | `81be4dd37746ae9e178c39e1b7b951c304a9c3cd2669ca64a90893955e922150` |
| `docs/LOCAL_MODELS_E2E_CHECKLIST.md` | 3,613 | `0dae53e5b642933240c36abcaa94795f54858e7a591cbea08c431238c920e394` |
| `docs/LOCAL_MODELS_LICENSES.md` | 1,506 | `4d29a30a1fdacb56591c65bbbd2efa092462fdf5efb171475f241daa365a85ed` |
| `server-0.1.33-darwin-arm64.tar.gz` | 170,785,353 | `ac2dc8dcbfc18e41ce53647f62a7a2fd5cedbcacbde344ce16fbb0844472e3aa` |
| `seed-train-darwin-arm64.json` | 875 | `78d5e36cf681cc7cb1248079e4f6c7d5d08838d1157077fd508ea8ce0c36103c` |
| `seed-train-darwin-arm64.json.sig` | 64 | `e5e42ace57215768e670a3bf319a2856a0b793a9dfb50afb6756a3e4dd14fceb` |
| `Lingxi.app/Contents/Resources/app.asar` | 40,806,499 | `5df99b01841e17c91ce16b802a410558eb7400ad7a3c78bffd9b4fb381592cd9` |

源码 ZIP 共 3,318 个文件，`zip -T` 通过。危险文件名扫描为 0；密钥特征只有两个已解释的脱敏测试夹具文件，内容分别是公开示例值和测试用私钥头，不是可用凭据。

复核命令：

```bash
shasum -a 256 /Users/study_superior/Downloads/LingxiAgent-local-models-codex-3eab8589-20260902.zip
zip -T /Users/study_superior/Downloads/LingxiAgent-local-models-codex-3eab8589-20260902.zip
```
