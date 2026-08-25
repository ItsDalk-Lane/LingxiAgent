## 一、任务性质

本任务针对 `ItsDalk-Lane/LingxiAgent` 当前桌面端主聊天界面进行一次**既有功能重新编排与必要的数据能力补充**。

本任务的核心不是新增一套新的聊天、工作台、终端、Skill、搜索或项目系统，而是：

1. 重新组织现有界面布局；
2. 将当前右侧工作台主体移动到左侧；
3. 将聊天记录限定到当前工作台；
4. 将运行期卡片整合为一个悬浮胶囊式信息中心；
5. 重排侧栏入口与输入区控制按钮；
6. 将聊天搜索改成顶部入口 + 居中搜索界面；
7. 为现有上下文用量增加真实的分类详情。

**禁止借本任务新增任何未要求的产品功能。**

---

# 二、必须遵守的范围边界

执行本任务时必须遵守以下原则。

## 2.1 只完成本任务明确要求的功能

不得自行增加：

* 新的工作台管理机制；
* 新的项目体系；
* 新的聊天标签体系；
* 新的收藏功能；
* 新的搜索筛选条件；
* 新的高级搜索；
* 新的聊天排序方式；
* 新的工作台排序方式；
* 新的左右栏布局模式；
* 可拖动上下分割线；
* 自定义上下区域比例；
* 新的终端功能；
* 新的子代理功能；
* 新的 Workflow 功能；
* 新的 Skill 执行功能；
* 新的上下文优化功能；
* 新的 Token 分析历史；
* 新的统计页面；
* 新的设置项；
* 新的快捷键；
* 新的移动端交互；
* 任何本任务没有明确要求的附加功能。

如果当前代码中某项功能已经存在，本任务允许为了重新布局而移动或复用它，但不得借机扩大其能力。

---

## 2.2 尽量保留现有业务逻辑

本任务优先改变：

* 组件编排；
* UI 容器；
* 数据筛选；
* 状态作用域；
* 上下文统计契约。

不应无理由重写：

* Session 生命周期；
* Workspace 生命周期；
* 文件管理业务；
* 搜索后端；
* Terminal 后端；
* Workflow 后端；
* Agent Activity 数据源；
* Skill 系统；
* Preview 系统；
* 消息发送协议。

---

## 2.3 桌面端为本任务目标

本任务主要针对桌面主聊天界面。

除非共享组件修改导致编译、类型检查或测试必须同步适配，否则：

* 不重新设计移动端；
* 不重新设计 Quick Chat；
* 不重新设计 Channel 页面；
* 不重新设计插件页面。

现有非目标界面行为应保持。

---

# 三、当前代码事实基线

执行前必须理解当前结构，不得把本任务当成纯 CSS 修改。

当前左侧 `ChatSidebar` 包含：

* “对话”标题；
* 新建聊天按钮；
* 设置按钮；
* 左侧栏折叠按钮；
* Bridge；
* Activity；
* Automation；
* Skills；
* `SessionList`。

这些入口目前是标题行和多条纵向 Activity Bar 的组合。

当前右侧 `RightWorkspacePanel` 同时包含两类不同性质的内容。

第一类是稳定的工作台内容：

* Workspace 标题；
* 项目技能；
* “对话文件 / 工作台”两个 Tab；
* Session Files；
* Desk / 工作台文件树；
* 工作台搜索、过滤、排序及已有文件操作；
* Plugin Widget 模式等既有工作台行为。

第二类是运行期内容：

* `SessionTodoCard`
* `TerminalCard`
* `WorkflowCard`
* `AgentActivityCard`
* `SessionStatusCard`
* `JianEditor` / 笺

它们现在被混在同一个右侧区域中。

本任务就是要把这两类内容正式拆开。

---

# 四、最终目标信息架构

完成以后，桌面聊天主界面应形成以下结构：

```text
┌──────────────────────────────────────────────────────────────┐
│ [左栏展开/折叠] [搜索]                  现有其他 Titlebar 控件 │
├───────────────────┬──────────────────────────────────────────┤
│ 左侧功能按钮行     │                                          │
│ 社交 活动 计划... │   [运行信息胶囊]                          │
├───────────────────┤                                          │
│                   │                                          │
│ 当前工作台聊天记录 │                                          │
│                   │               当前聊天                   │
│      50%          │                                          │
│                   │                                          │
├───────────────────┤                                          │
│ 当前工作台标题     │                                          │
│ 对话文件 | 工作台 │                                          │
│ 工作台搜索/文件树 │                                          │
│                   │                                          │
│      50%          │                                          │
├───────────────────┼──────────────────────────────────────────┤
│                   │ ┌──────────────────────────────────────┐ │
│                   │ │ 输入内容                      [发送] │ │
│                   │ └──────────────────────────────────────┘ │
│                   │ [新聊][附件][命令][权限][上下文]...[模型]│
└───────────────────┴──────────────────────────────────────────┘
```

原来永久占据最右侧的工作区 Rail 不再存在。

---

# 五、任务一：把右侧稳定工作台主体移动到左侧下半区

## 5.1 拆分现有 RightWorkspacePanel

不能继续把 `RightWorkspacePanel` 当成一个不可拆分整体直接搬到左边。

需要先将其拆分为：

### A. 稳定工作台主体

包含：

* Workspace Header；
* 工作台名称；
* 项目技能按钮；
* 项目技能现有面板；
* “对话文件”Tab；
* “工作台”Tab；
* `SessionRegistryFilesPanel`；
* `DeskSection`；
* 工作台搜索；
* 文件树；
* 文件过滤；
* 文件排序；
* Preview/Open 等现有操作；
* 当前已经存在的工作台 Plugin Widget 行为。

这一部分移动到左侧栏下半区。

### B. 会话运行信息

包括：

* 笺；
* Todo；
* Terminal；
* Workflow / 运行命令；
* Agent Activity / 子代理；
* Session Status。

这些不得随工作台主体进入左侧文件区。

它们进入后文定义的“运行信息悬浮胶囊”。

---

## 5.2 工作台主体移动后必须保持原行为

除了位置和适应新容器所必须的尺寸修改之外：

* 不修改工作台 Tab 含义；
* 不修改工作台标题逻辑；
* 不修改“项目技能”行为；
* 不修改文件树操作；
* 不修改搜索工作台行为；
* 不修改过滤行为；
* 不修改排序行为；
* 不修改文件拖放；
* 不修改文件右键菜单；
* 不修改 Preview/Open；
* 不修改 Session Files；
* 不修改文件数据源。

原则是：

> 当前右侧工作台稳定主体，除运行期内容外，整体原样迁移。

---

# 六、任务二：左侧栏改为上下 50% 双区

## 6.1 左侧栏内部结构

新的聊天状态下左栏必须由三部分构成：

### 第一部分：顶部功能按钮栏

高度根据现有控件自然决定，不参与 50/50 计算。

### 第二部分：聊天记录区

占顶部功能栏之外剩余空间的 50%。

### 第三部分：工作台/文件区

占剩余空间的另外 50%。

即：

```text
左栏总高度
- 顶部功能按钮栏
= 剩余有效高度

聊天记录 = 有效高度 50%
工作台   = 有效高度 50%
```

---

## 6.2 不增加可调比例功能

用户没有要求上下区域可拖动调整。

因此：

* 不增加 Resize Handle；
* 不增加比例持久化；
* 不增加“折叠聊天区”；
* 不增加“折叠工作台区”；
* 不增加自定义比例。

两个区域固定平分可用高度。

---

## 6.3 两个区域独立滚动

如果聊天记录或文件内容超过各自区域高度：

* 聊天记录区内部滚动；
* 工作台区内部滚动。

不得因为其中一侧内容过多而把另一侧挤出视口。

布局必须正确处理：

* `flex: 1`
* `min-height: 0`
* 内部 overflow

避免出现整个左栏页面级滚动。

---

# 七、任务三：移除原固定右侧工作区

当前 Workspace Companion / Right Workspace Rail 不再作为聊天页面永久固定的一列。

完成以后：

* 图一所示稳定主体已经进入左栏；
* 运行期信息已经进入浮动胶囊；
* 因此原右 Workspace Rail 在 Chat 页面不应继续占宽度。

不得出现：

```text
左侧新工作台
+
右侧旧工作台
```

这种重复 UI。

---

## 7.1 原右栏展开/折叠入口

当前 Titlebar 中与原固定右工作区直接绑定的右侧栏开关，如果失去对应固定 Rail 后已经没有意义，则应移除其聊天页面上的对应 UI 和无效交互。

但不得因此删除仍被其他界面实际依赖的业务状态。

应清理的是：

* 已不再存在的固定右栏展示控制；
* 已失效的 hover 浮出右栏逻辑；
* 为旧右栏布局服务的 CSS。

不得顺便删除：

* 文件功能；
* Workspace 状态；
* Preview；
* Plugin Widget 功能。

---

# 八、任务四：创建统一“运行信息悬浮胶囊”

## 8.1 位置

运行信息中心悬浮在：

> 主聊天内容区域左上角。

它不属于左 Sidebar。

它也不属于 Preview。

它覆盖在主 Chat Surface 上方。

---

## 8.2 收起状态

视觉和交互参考用户提供的图二。

要求：

* 小型胶囊形态；
* 大圆角；
* 不占据聊天正常文档流空间；
* 点击即可展开；
* 不因为展开/收起改变 Chat Transcript 宽度；
* 不把主聊天内容向下推。

收起状态只承担入口和简要状态展示。

不得在胶囊外再同时显示独立运行卡片。

---

# 九、任务五：运行信息展开面板

## 9.1 展开样式

参考用户提供的图三。

展开后必须是：

> 一个统一圆角悬浮容器。

不是若干彼此分离的悬浮卡片。

容器内部可以按 Section 组织当前运行信息。

---

## 9.2 当前需要纳入的既有内容

将当前右工作区中的：

* Jian / 笺；
* Session Todo；
* Terminal；
* Workflow；
* Agent Activity；
* Session Status；

移动到统一运行信息容器。

---

## 9.3 内容业务保持不变

例如：

Terminal 仍使用当前 Terminal 数据与操作。

Workflow 仍使用当前 Workflow 数据。

Agent Activity 仍使用当前 ActivityHub/WS 数据。

Session Status 仍使用现有状态。

Todo 仍使用当前 Session Todo。

笺仍编辑当前现有 Jian 内容。

不得为了这个浮层重新制作第二套状态模型。

---

## 9.4 Skill 的边界

这里必须区分三种 Skill UI：

### 第一种：左侧工作台里的“项目技能”

这是 Workspace 稳定功能。

继续留在迁移后的工作台主体。

### 第二种：左侧顶部全局 Skill 入口

这是现有全局 Skills Panel 的入口。

继续保留，只改成纯图标。

### 第三种：未来可能存在的运行中 Skill 卡片

这类属于运行期信息，应进入运行信息胶囊。

但是：

**如果当前仓库并没有这样一张真实运行时 Skill 卡片，本任务不得为了“预留”而新建假 Skill 卡片、假数据或新的 Skill 执行功能。**

未来新增同类卡片时应使用该统一容器，但不属于本任务新增功能范围。

---

# 十、任务六：左侧顶部功能入口重构

当前 Sidebar Header 中：

* 对话标题；
* 新聊天；
* 设置；
* 折叠；

需要重构。

---

## 10.1 删除“对话”标题

不再显示：

```text
对话
```

或者其国际化等价标题。

这行不再承担页面标题职责。

---

## 10.2 删除 Sidebar 内新建聊天按钮

当前 Sidebar Header 中的“+”新聊天按钮移除。

新建聊天入口将在 Composer 中重新安置。

---

## 10.3 删除 Sidebar 内折叠按钮

因为应用最顶部已经有左侧栏展开/折叠入口，所以 Sidebar 内部的重复折叠按钮移除。

不得保留两个功能相同的左栏折叠按钮。

---

## 10.4 保留设置

设置入口继续存在。

功能保持不变。

---

## 10.5 四项功能改为纯图标

当前：

* 社交平台助手 / Bridge；
* 活动 / Activity；
* 活动任务计划 / Automation；
* Skill；

不再分别占用完整横向 Activity Bar。

改成纯图标按钮。

与设置按钮一起放入同一个顶部功能按钮行。

例如结构概念为：

```text
[Bridge] [Activity] [Automation] [Skill]       [Settings]
```

具体 spacing 应服从现有 UI 风格。

---

## 10.6 保留现有状态提示

如果当前某入口已有状态信息，例如：

* Bridge 在线状态点；
* Automation 数量 Badge；

在改成图标后仍应继续表达。

但不得增加新的 Badge 类型。

---

# 十一、任务七：聊天记录只显示当前工作台会话

这是本任务的核心数据行为修改之一。

---

## 11.1 左栏不再展示全局 Session 集

常驻聊天记录区域只显示：

> 当前 Workspace 对应的 Session。

不能继续把所有工作台会话混在一起。

---

## 11.2 Workspace 作为作用域

当前工作台必须成为：

* 文件浏览器的 Scope；
* 左侧聊天列表的 Scope。

二者必须同步。

---

## 11.3 工作台切换结果

假设存在：

```text
Workspace A
Workspace B
```

当前为 A 时：

```text
聊天列表 -> A 的 Session
文件树   -> A 的文件
```

切换 B 后：

```text
聊天列表 -> B 的 Session
文件树   -> B 的文件
```

两者必须属于同一个工作台身份。

---

# 十二、任务八：Workspace 与 Session 的匹配规则

不得通过：

* Session 标题；
* Workspace 显示名称；
* Agent 名称；
* projectId 名称；

进行模糊匹配。

必须使用已有稳定身份字段。

---

## 12.1 Studio / Mount Workspace

当当前工作台具有 `workspaceMountId` 时：

优先通过：

```text
session.workspaceMountId === activeWorkspaceMountId
```

确定归属。

---

## 12.2 普通本地目录 Workspace

没有 Mount ID 时：

使用经过项目现有规范化规则处理后的 `cwd` 与当前工作台 root/cwd 做身份比较。

不得用简单字符串显示名称进行比较。

必须考虑当前代码已经存在的路径规范化规则。

---

## 12.3 projectId 不再作为左栏主作用域

Session 中的 `projectId` 可以继续存在，因为其他历史数据和兼容逻辑可能仍然需要。

但左侧常驻聊天记录的主 Scope 不再由 `projectId` 决定。

本任务不是“把 projectId 改名成 workspaceId”。

---

## 12.4 无法归属当前 Workspace 的 Session

不得偷偷把没有可靠 Workspace 身份的旧 Session 自动归入当前 Workspace。

它们：

* 不出现在错误工作台的常驻聊天列表；
* 数据本身不得删除；
* 仍可通过后文的全局聊天搜索找到。

本任务不做 Session 数据迁移工程。

---

# 十三、任务九：移除左栏“项目 / 时间”主导航

当前 SessionList 存在时间视图与 Project 视图。

新的左侧常驻聊天列表不再承担跨所有 Workspace 的 Project 管理职责。

因此需要从常驻左栏移除：

* 时间 / 项目视图切换；
* “项目”一级标题；
* 添加项目按钮；
* 项目目录式总览；
* 在左栏通过项目浏览全部 Workspace Session 的入口。

---

## 13.1 不等于删除底层 Project 数据

本任务要求的是：

> 不再让 Project 成为左侧聊天主导航。

不是要求大规模删除项目数据结构、迁移历史数据或删除后端 Project API。

如果现有兼容逻辑仍需保留，可以保留。

但新左栏 UI 不再暴露这一套导航。

---

# 十四、任务十：聊天搜索从 SessionList 抽离

当前 Session 搜索框不再永久显示在左侧聊天列表上方。

---

## 14.1 左侧列表取消常驻搜索框

左侧聊天记录区域只负责展示当前 Workspace 的 Session。

顶部不再永久占用一条搜索输入框。

---

# 十五、任务十一：Titlebar 增加聊天搜索按钮

在应用第一行，即现有：

> 左侧栏展开 / 折叠按钮

旁边增加一个：

> 放大镜图标按钮。

按钮只显示图标。

不显示“搜索聊天记录”等常驻文字。

---

# 十六、任务十二：新增居中聊天搜索界面

点击 Titlebar 放大镜后：

显示一个位于整个应用中央的聊天搜索界面。

---

## 16.1 搜索界面结构

至少包含：

```text
┌──────────────────────────┐
│ 🔍 搜索聊天记录          │
├──────────────────────────┤
│ Chat A                   │
│ Chat B                   │
│ Chat C                   │
│ ...                      │
└──────────────────────────┘
```

---

## 16.2 未输入关键词

搜索框为空时：

下方显示全部聊天记录。

这里是：

> 全局聊天记录。

不是当前 Workspace 限定列表。

---

## 16.3 输入关键词

继续复用当前已有 Session Search 能力。

不要重新实现第二套搜索后端。

现有标题搜索和内容搜索机制继续使用。

---

## 16.4 点击结果

点击搜索结果应继续进入对应 Session。

如果搜索结果来自其他 Workspace：

切换 Session 后应通过现有 Session → Workspace 恢复机制，使：

* 当前聊天切换；
* 当前 Workspace 跟随；
* 左侧文件区跟随；
* 左侧聊天 Scope 跟随。

不得只打开聊天而让工作台仍停留在错误目录。

---

## 16.5 搜索不增加额外能力

本任务不增加：

* 日期筛选；
* Workspace 筛选；
* Agent 筛选；
* Model 筛选；
* 高级搜索语法；
* 搜索历史；
* 最近搜索；
* 搜索收藏。

---

# 十七、任务十三：把新建聊天入口移动到 Composer

Sidebar 内新聊天入口删除后，在聊天 Composer 区域增加新的新建聊天按钮。

---

## 17.1 位置

位于附件按钮之前。

最终顺序：

```text
[新建聊天] [附件] ...
```

---

## 17.2 行为

必须调用现有 Session 创建流程。

继续复用：

```text
createNewSession(...)
```

或者其最终统一入口。

不得实现第二套 Session 创建状态机。

---

## 17.3 图标

当前“+”号不能继续承担新聊天图标。

改为明确表达：

> 新建消息 / 新聊天

的消息类图标。

不增加文字按钮。

---

# 十八、任务十四：修改附件图标

现有附件入口仍保留原功能。

只修改视觉语义：

原：

```text
+
```

改为：

```text
回形针 / Attachment
```

不得改变：

* 文件选择；
* 粘贴；
* 拖放；
* 上传；
* Session File；
* 媒体附件能力。

---

# 十九、任务十五：重构 InputControlBar

当前 Input Control Bar 中包含大量控制，而且全部在输入卡片内部。

需要重新划分为：

### 输入卡片内部

只保留：

* 文本编辑区域；
* 发送按钮。

### 输入卡片外部、聊天页面底部 Toolbar

移动：

* 新建聊天；
* 附件；
* Slash / Command Menu；
* Permission / Plan Mode；
* Context Ring；
* Thinking Level；
* Model Selector；
* Audio / Voice Input。

---

# 二十、任务十六：底部 Composer Toolbar

新的 Toolbar 位于：

> 输入卡片下面。

属于当前 Chat Page 底部区域。

它不是整个 Electron 窗口全局 Status Bar。

---

## 20.1 Toolbar 不应影响发送按钮

发送按钮继续位于输入卡片中。

流式输出期间现有：

* Stop；
* Steer；
* Send 状态切换；

仍保持当前业务行为。

仅改变布局位置，不重新设计发送协议。

---

## 20.2 输入区其他现有内容

以下内容不是本次“按钮移动”的目标：

* Attached File 展示；
* Quote；
* Session Confirmation；
* Slash Result；
* Error Status；
* Recording Status；
* Mention Menu；
* Slash Menu。

这些应按现有业务逻辑继续显示在合理的输入相关位置。

不得为了“输入框简化”把这些功能删除。

---

# 二十一、任务十七：Context Ring 增加“详情”

当前 Context Ring 功能继续保留。

现有：

* Context 使用比例；
* Token 总量；
* Compact；
* Refresh and Compact；
* 实验性 Compact 选项；

不得删除。

---

## 21.1 菜单新增一个“详情”

点击 Context Ring 后的菜单增加：

```text
详情
```

点击后展示当前 Session 的 Context 使用组成。

---

# 二十二、任务十八：Context Detail 数据要求

不能只做一个前端弹窗然后根据字符串长度伪造 Token。

详情必须来自：

> 当前真正进入主聊天模型上下文的数据。

---

## 22.1 至少需要区分

根据实际请求中存在的数据来源，显示：

* System Prompt；
* Conversation / 历史消息；
* 当前 User Input；
* Tool Definitions；
* Tool Results；
* MCP；
* Skill；
* 文件 / 引用 / Workspace Context；
* 其他真实上下文开销。

如果某类当前不存在：

可以显示 0 或不显示。

不得制造不存在的上下文项。

---

## 22.2 Skill，不是 SQL

需求中的对应分类明确为：

```text
Skill
```

不得实现 SQL 分类。

---

## 22.3 总量

详情应同时能看到：

* 已使用 Token；
* Context Window 总量；
* 剩余容量；
* 各分类 Token；
* 各分类占已使用 Context 的比例。

这些只是现有 Context 使用情况的展开详情，不扩展成独立统计系统。

---

# 二十三、任务十九：Context Breakdown 的统计位置

分类 Token 不能在 React 层重新猜。

应定位：

> 主聊天模型最终 Context 构建完成的位置。

在最终请求真正发送给 Provider 前，对组成上下文的来源进行分类统计。

---

## 23.1 分类必须对应真实来源

推荐原则：

```text
Context Item
→ source category
→ token count
→ aggregate
```

例如：

```text
system messages     -> system
conversation turns  -> conversation
current prompt       -> user
tool schemas         -> tools
tool outputs         -> tool_results
MCP injections       -> mcp
skills               -> skills
file context         -> files
无法单独归类的开销   -> other
```

---

## 23.2 不允许前端字符数估算

禁止：

```text
字符数 / 4
```

之类粗略估算后宣称为真实 Token。

必须复用当前模型/运行时可用的 Token 统计能力，或在最终 Context 构造层获得可信统计。

---

## 23.3 Breakdown 与总量关系

必须保证：

```text
sum(categories) + unavoidable overhead
≈ current contextTokens
```

如果存在协议或 tokenizer 无法单独拆分的部分：

统一放入：

```text
other / overhead
```

而不是让明细和总数明显对不上。

---

# 二十四、任务二十：扩展现有 context_usage 契约

优先扩展当前已有的 Context Usage 通路。

现有前端已经通过 WS 请求：

```text
context_usage
```

服务端返回：

```text
tokens
contextWindow
percent
```

本任务应优先把 Breakdown 作为该能力的扩展字段，而不是无理由再设计第二套并行系统。

建议兼容结构：

```text
context_usage
├── tokens
├── contextWindow
├── percent
└── breakdown
```

`breakdown` 为新增可选字段。

这样：

* 现有 Context Ring 继续使用总量；
* 新 Detail 使用 Breakdown；
* 不复制状态通路。

---

# 二十五、任务二十一：Context Detail 生命周期

详情必须属于当前 Session。

切换 Session 后：

* 旧 Session 明细不能继续显示；
* 当前 Context Ring 总量跟随 Session；
* Detail 同样跟随 Session。

请求、响应和 Store 均必须使用当前项目已有的 Session Identity 机制。

禁止只使用一个全局 Breakdown 状态而导致 A/B Session 串数据。

---

# 二十六、任务二十二：Float Sidebar 适配

当前项目存在折叠后 hover 出现的 Float Sidebar。

本次左侧结构已经改变，因此必须检查 Float Sidebar。

---

## 26.1 左侧浮出时

如果继续保留现有左侧折叠 hover 行为：

浮出的左侧内容必须反映新的左侧结构：

* 顶部功能图标行；
* 当前 Workspace Session；
* 当前 Workspace 工作台文件区。

不得 hover 出旧版 Sidebar。

---

## 26.2 右侧旧 Float Panel

如果它只是为了原 `RightWorkspacePanel` 折叠后的 hover 展示服务，那么在固定右工作区被移除后：

清理这条已经失去意义的展示路径。

不得留下：

* 空的右浮层；
* 重复工作台；
* hover 后又出现旧右工作台。

---

# 二十七、任务二十三：PreviewPanel 保持独立

本任务不要求把 PreviewPanel 并入左栏。

Preview 继续使用现有 Preview 系统。

不得：

* 把 Preview 塞进 50% 工作台区域；
* 删除 Preview；
* 改写 Preview 编辑器；
* 改 Preview Tab 体系。

本任务只移除旧固定 Workspace Rail。

---

# 二十八、任务二十四：Workspace 切换的一致性

切换 Workspace 时必须形成一个原子级用户体验。

最终 UI 必须同时对齐：

```text
active workspace
├── workspace header
├── workspace files
└── session list
```

不能出现中间状态长期错配：

```text
Workspace Header = B
Files = B
Sessions = A
```

---

# 二十九、任务二十五：切换 Session 的反向同步

反向也必须成立。

当用户从：

* 全局聊天搜索；
* 通知；
* 其他已有 Session 导航入口；

打开一个属于 Workspace B 的 Session 时：

最终 UI 必须恢复为：

```text
当前 Session = B 中的 Session
当前 Workspace = B
左侧 Session List = B
左侧文件树 = B
```

继续复用当前 `switchSession()` 与 Workspace 激活流程，不建立另一条独立路径。

---

# 三十、任务二十六：新会话与 Workspace 作用域

点击 Composer 中的新建聊天按钮时：

新会话继续进入当前项目已有的 Pending Session 流程。

不得因为聊天列表已经 Workspace Scoped 而创建另一套“Workspace Session Create”。

现有 Session 创建时已有的：

* selectedFolder；
* workspaceMountId；
* workspaceLabel；
* pendingProjectId 等；

继续由现有 Session 创建逻辑管理。

---

# 三十一、任务二十七：CSS 与布局清理

重构后应删除或停止使用：

* 旧固定 Right Workspace Rail 专属布局；
* 旧 Sidebar Activity Bar 纵向布局；
* 旧 Sidebar Header 中删除元素的样式；
* 旧输入框内部 Bottom Bar 布局；
* 失效的右栏 hover 相关样式。

但只删除确定已经失去引用的样式。

不要进行全仓 CSS 大扫除。

---

# 三十二、任务二十八：响应式边界

本任务不新增响应式模式。

桌面窗口变窄时必须至少保证：

* 左栏仍可正常滚动；
* 50/50 两区不会把彼此挤没；
* 主聊天输入框不溢出；
* Toolbar 可以按照当前项目已有设计方式适配有限宽度；
* 悬浮运行信息不遮死整个聊天区。

不要因此添加新的“紧凑模式设置”。

---

# 三十三、任务二十九：i18n

新增真正需要展示给用户的文本时必须进入现有国际化体系。

例如：

* Context “详情”；
* Context Breakdown 分类名称；
* 运行信息容器必要标题；
* 搜索界面必要文案。

已有文案应优先复用现有 key。

不得在组件内批量硬编码中文。

同时不得为了本任务重构整个 i18n 系统。

---

# 三十四、必须重点修改/审查的现有区域

实施时至少检查以下当前模块及其直接依赖：

```text
desktop/src/react/App.tsx
desktop/src/react/components/app/AppTitlebar.tsx
desktop/src/react/components/app/AppPages.tsx
desktop/src/react/components/app/ChatSidebar.tsx
desktop/src/react/components/app/WorkspaceCompanionRail.tsx

desktop/src/react/components/FloatSidebar.tsx

desktop/src/react/components/SessionList.tsx
desktop/src/react/components/session-sections.ts
desktop/src/react/stores/session-actions.ts
desktop/src/react/stores/desk-actions.ts

desktop/src/react/components/right-workspace/RightWorkspacePanel.tsx
desktop/src/react/components/right-workspace/RightWorkspacePanel.module.css
desktop/src/react/components/right-workspace/SessionTodoCard.tsx
desktop/src/react/components/right-workspace/TerminalCard.tsx
desktop/src/react/components/right-workspace/WorkflowCard.tsx
desktop/src/react/components/right-workspace/AgentActivityCard.tsx
desktop/src/react/components/right-workspace/SessionStatusCard.tsx

desktop/src/react/components/DeskSection.tsx
desktop/src/react/components/desk/DeskEditor.tsx
desktop/src/react/components/desk/DeskCwdSkills.tsx

desktop/src/react/components/InputArea.tsx
desktop/src/react/components/input/InputControlBar.tsx
desktop/src/react/components/input/ContextRing.tsx
desktop/src/react/components/input/InputArea.module.css

desktop/src/react/services/ws-message-handler.ts
desktop/src/react/services/websocket.ts
desktop/src/react/stores/context-slice.ts

server/routes/chat.ts
```

这不是要求所有文件都必须修改。

原则是：

> 先定位责任，再修改最小必要集合。

---

# 三十五、推荐实施顺序

必须按照依赖关系实施，避免先改 UI 再发现状态模型无法支撑。

## Phase 1：拆分 RightWorkspacePanel

完成：

* 稳定 Workspace 主体与 Runtime 卡片解耦；
* 不改变任何行为；
* 为后续迁移准备独立组件边界。

验收后再继续。

---

## Phase 2：左 Sidebar 建立上下双区

完成：

* 顶部工具行容器；
* Session 区；
* Workspace 区；
* 固定 50/50；
* 两区独立滚动。

此阶段可以暂时仍显示旧 Session 全量，随后 Phase 3 改 Scope。

---

## Phase 3：Session List 改为 Workspace Scoped

完成：

* Active Workspace 身份解析；
* Session 归属判断；
* 当前工作台过滤；
* Workspace 切换同步；
* Session 切换反向同步。

同时移除常驻 Project/Time 主导航 UI。

---

## Phase 4：迁移 Workspace 主体并删除旧 Right Rail

完成：

* Workspace 主体正式进入左栏；
* 旧固定 Workspace Rail 不再渲染；
* 清理旧右栏 toggle / float-right 展示路径；
* Preview 保持。

---

## Phase 5：实现运行信息胶囊

完成：

* Chat 左上角固定悬浮位置；
* Collapse Capsule；
* Expanded Container；
* Jian；
* Todo；
* Terminal；
* Workflow；
* Agent Activity；
* Session Status。

只移动既有功能。

---

## Phase 6：重构 Sidebar 顶部入口

完成：

* 删除“对话”；
* 删除 Sidebar 新聊天；
* 删除 Sidebar 折叠；
* Bridge 图标；
* Activity 图标；
* Automation 图标；
* Skill 图标；
* Settings 图标；
* 保留已有状态 Badge/Dot。

---

## Phase 7：重构聊天搜索

完成：

* SessionList 常驻搜索框移除；
* Titlebar 放大镜；
* 居中搜索界面；
* 空查询显示全部 Session；
* 查询继续复用现有 Search API；
* 点击 Session 正确恢复 Workspace。

---

## Phase 8：Composer 布局重构

完成：

* 新聊天按钮进入附件之前；
* 新聊天图标改为消息语义；
* 附件改为回形针；
* Send 留在输入卡片；
* 其他所有 Control 移到输入框下方 Toolbar；
* 保持原业务行为。

---

## Phase 9：Context Usage Breakdown

完成：

* 找到真实 Context 构造边界；
* 标记 Context 来源；
* 真实 Token 分类；
* 扩展 `context_usage`；
* Store 按 Session 保存；
* Context Ring 菜单增加“详情”；
* Detail UI。

---

## Phase 10：清理与回归

仅清理这次重构造成的：

* Dead UI；
* Dead imports；
* Dead CSS；
* 旧右 Rail 接线；
* 重复 Sidebar 路径。

不要顺手重构无关模块。

---

# 三十六、功能验收标准

任务只有满足以下全部条件才算完成。

## 36.1 左侧布局

* [ ] 左侧顶部是纯功能按钮行。
* [ ] 不再显示“对话”标题。
* [ ] Sidebar 内没有重复新聊天按钮。
* [ ] Sidebar 内没有重复折叠按钮。
* [ ] Bridge、Activity、Automation、Skill、Settings 在同一行。
* [ ] Bridge/Automation 原有状态提示仍工作。
* [ ] Chat Session 区占有效高度 50%。
* [ ] Workspace 区占有效高度 50%。
* [ ] 两区独立滚动。

---

## 36.2 Workspace

* [ ] 原右 Workspace 稳定主体移动到左下。
* [ ] “项目技能”保留。
* [ ] 对话文件 Tab 保留。
* [ ] 工作台 Tab 保留。
* [ ] 工作台搜索保留。
* [ ] 文件树保留。
* [ ] 过滤保留。
* [ ] 排序保留。
* [ ] 原有文件操作保留。
* [ ] 没有第二份重复 Workspace UI。

---

## 36.3 右栏

* [ ] Chat 页面不再存在原固定 Right Workspace Rail。
* [ ] 不再为旧 Right Workspace Rail 永久预留宽度。
* [ ] 不存在无效右栏 Toggle。
* [ ] 不存在 hover 后重新出现旧 Workspace 的路径。
* [ ] Preview 未被删除或并入左栏。

---

## 36.4 Runtime Capsule

* [ ] 位于主聊天左上角。
* [ ] 收起为胶囊。
* [ ] 点击可展开。
* [ ] 展开后为一个统一面板。
* [ ] Jian 已进入。
* [ ] Todo 已进入。
* [ ] Terminal 已进入。
* [ ] Workflow 已进入。
* [ ] Agent Activity 已进入。
* [ ] Session Status 已进入。
* [ ] 不存在原位置重复卡片。
* [ ] 没有凭空新增 Runtime Skill 假卡片。

---

## 36.5 Session Scope

准备两个不同 Workspace：

```text
A
B
```

且各自存在 Session。

验收：

* [ ] A 时只显示 A Session。
* [ ] A 时文件区显示 A 文件。
* [ ] 切 B 后只显示 B Session。
* [ ] 切 B 后文件区显示 B 文件。
* [ ] 两个区域不会错配。
* [ ] 不依赖显示名称匹配。
* [ ] Mount Workspace 按 Mount ID。
* [ ] Local Workspace 按规范化 cwd。
* [ ] projectId 不再控制左侧常驻列表。

---

## 36.6 搜索

* [ ] 左栏没有永久搜索框。
* [ ] Titlebar 左侧栏 Toggle 旁有放大镜。
* [ ] 点击打开居中搜索界面。
* [ ] 空查询显示全部聊天记录。
* [ ] 输入关键词可搜索。
* [ ] 仍能搜索标题。
* [ ] 仍能搜索正文。
* [ ] 搜索结果可进入 Session。
* [ ] 进入其他 Workspace Session 后 Workspace 同步恢复。
* [ ] 未新增高级搜索功能。

---

## 36.7 Composer

* [ ] 输入框内只保留输入主体与 Send 主控制。
* [ ] 新聊天在输入框外 Toolbar。
* [ ] 新聊天在附件之前。
* [ ] 新聊天使用消息语义图标。
* [ ] 附件使用回形针。
* [ ] Slash 已移出输入卡片。
* [ ] Permission/Plan 已移出输入卡片。
* [ ] Context Ring 已移出输入卡片。
* [ ] Thinking 已移出输入卡片。
* [ ] Model Selector 已移出输入卡片。
* [ ] Audio 已移出输入卡片。
* [ ] Send/Stop/Steer 原行为正常。
* [ ] 附件、媒体发送等业务没有被破坏。

---

## 36.8 Context Detail

* [ ] Context Ring 原总量仍正常。
* [ ] 原 Compact 操作仍正常。
* [ ] 增加“详情”。
* [ ] Detail 属于当前 Session。
* [ ] System Prompt 有真实 Token 数据。
* [ ] Tools 有真实 Token 数据。
* [ ] User Input 有真实 Token 数据。
* [ ] MCP 有真实 Token 数据。
* [ ] Skill 有真实 Token 数据。
* [ ] 其他实际 Context 来源正确分类。
* [ ] 没有 SQL 分类。
* [ ] 不是前端字符数估算。
* [ ] Breakdown 总和与总体 Context 使用基本闭合。
* [ ] Session A/B Detail 不串数据。

---

# 三十七、回归测试要求

除现有完整测试外，必须至少补充或调整覆盖以下行为的测试。

## Sidebar

测试：

* 顶部标题被移除；
* 新聊天被移除；
* 折叠按钮被移除；
* 四个入口 + Settings 正常；
* Button callback 正确。

## Workspace Scope

测试：

* workspaceMountId 匹配；
* cwd 匹配；
* Workspace 切换后 Session 过滤；
* 无 Workspace 身份 Session 不错误混入。

## Runtime Container

测试：

* Collapse；
* Expand；
* Session 切换；
* Terminal/Workflow/Activity 等仍绑定当前 Session。

## Search

测试：

* Titlebar 搜索按钮；
* 空搜索显示所有 Session；
* 查询结果；
* 点击切 Session；
* 跨 Workspace Session 恢复。

## Composer

测试：

* New Session；
* Attachment；
* Slash；
* Permission；
* Context；
* Thinking；
* Model；
* Audio；
* Send。

重点验证：

> 只是位置改变，行为没有丢失。

## Context

测试：

* 旧 `context_usage` 总量字段；
* Breakdown；
* Session identity；
* 详情 UI；
* 缺失 Breakdown 时不会破坏原 Context Ring。

---

# 三十八、禁止通过“隐藏”伪装完成

以下方式不算完成：

### 错误方式 1

用 CSS 把旧 Right Workspace 隐藏，然后在左边复制一份。

必须移除重复渲染路径。

### 错误方式 2

左边 Session List 仍加载全部 Session，只通过视觉遮盖其他 Workspace。

必须在展示数据层实现 Workspace Scope。

### 错误方式 3

Context Detail 在前端根据字符串长度估算。

必须使用实际 Context 构造来源。

### 错误方式 4

运行卡片仍各自独立存在，只在外面包一个透明 div。

需要形成真正统一的运行信息容器。

### 错误方式 5

保留旧项目导航，只默认隐藏。

常驻左栏新的信息架构不再依赖 Project View。

---

# 三十九、不得破坏的既有功能

完成本任务后必须确保至少以下功能仍可正常工作：

* 创建新会话；
* 切换会话；
* Session streaming；
* 多 Session 后台流；
* 文件附件；
* 图片输入；
* 视频输入；
* 音频输入；
* 文件树；
* Session Files；
* 工作台搜索；
* 工作台过滤；
* 工作台排序；
* Preview；
* Jian 保存；
* Terminal；
* Workflow；
* Agent Activity；
* Session Todo；
* Session Status；
* Bridge；
* Activity Panel；
* Automation Panel；
* Skills Panel；
* Context Compact；
* Context Refresh；
* 模型切换；
* Thinking Level；
* Permission Mode；
* Slash Command。

---

# 四十、交付要求

实现完成后，执行者需要提交一份结果说明，至少包含：

## 40.1 实际修改文件

列出所有修改文件，并说明其责任。

## 40.2 布局变化

明确说明：

```text
旧：
Sidebar | Chat | Preview | RightWorkspace

新：
Sidebar(Chat + Workspace) | Chat | Preview
                          + Runtime Floating Capsule
```

## 40.3 Workspace Scope

说明最终如何判断：

```text
active workspace
→ matching sessions
```

尤其说明：

* Mount Workspace；
* Local cwd Workspace。

## 40.4 Context Breakdown

说明：

* 数据在哪里统计；
* 使用什么 Token 统计来源；
* 如何分类；
* 如何通过 WS 传递；
* 如何按 Session 缓存；
* 如何保证 Breakdown 与总量一致。

## 40.5 测试

给出：

* typecheck；
* 相关单元测试；
* UI/组件测试；
* 完整项目测试；

的执行结果。

---

# 四十一、最终完成定义

本任务的最终目标只有以下几件事：

1. **工作台文件区域从右侧移动到左侧聊天记录下面。**
2. **聊天记录和文件区上下各占左栏有效高度的一半。**
3. **左侧聊天记录只显示当前工作台的会话。**
4. **原右侧固定 Workspace Rail 被移除。**
5. **笺、Todo、Terminal、Workflow、子代理活动、Session 状态统一进入聊天左上角的胶囊式运行信息面板。**
6. **Bridge、Activity、Automation、Skill、Settings 收敛为左侧顶部同一行纯图标按钮。**
7. **搜索框从左栏删除，改成 Titlebar 放大镜 + 居中全局聊天搜索。**
8. **新聊天按钮移动到 Composer，并位于附件前。**
9. **新聊天图标改成消息图标，附件改成回形针。**
10. **除发送按钮外，输入框底部所有控制移到输入框之外的底部 Toolbar。**
11. **Context Ring 增加详情，并真实展示 System、Tools、User Input、MCP、Skill 等 Context Token 占用。**

除此之外，**不要增加任何其他功能。**

如果实现过程中发现需求之外的问题，除非它阻塞上述功能或属于此次修改直接造成的回归，否则不要顺便修复；记录即可，不扩大本任务范围。
