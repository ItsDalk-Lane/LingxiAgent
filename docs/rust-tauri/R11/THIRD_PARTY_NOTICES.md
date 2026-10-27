# R11 随包第三方组件说明

## Chromium（macOS arm64，snapshot revision 1727675）

随包路径：`Lingxi.app/Contents/Resources/chromium/Chromium.app`

来源：Chromium 官方快照 `chromium-browser-snapshots`（`Mac_Arm/1727675`）。
版本报告：`Chromium 159.0.8128.0`。

Chromium 以 BSD 3-Clause 风格许可分发。以下为 Chromium 项目 LICENSE 的标准文本
（完整第三方依赖清单见 Chromium 源码树与 `Chromium.app/Contents/Resources` 内
许可文件；随包只保留运行所需资源）：

```
Copyright 2015 The Chromium Authors. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

   * Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.
   * Redistributions in binary form must reproduce the above
copyright notice, this list of conditions and the following disclaimer
in the documentation and/or other materials provided with the
distribution.
   * Neither the name of Google Inc. nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

正式对外分发前需由业主/法务确认随包许可与体积策略；本文件只记录当前实现。


## Node.js（macOS arm64，v24.16.0）

随包路径：`Lingxi.app/Contents/Resources/node/bin/node`

Node.js 以 MIT 许可分发；随包已包含 `Resources/node/LICENSE`。官方便携包
同时包含 OpenSSL、ICU、V8 等第三方组件，其许可文本见 Node 源码树
`LICENSE` 与 `deps/` 各组件目录；正式对外分发前需由业主/法务确认。
