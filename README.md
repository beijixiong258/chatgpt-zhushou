# chatgpt对话助手

chatgpt对话助手是面向 Windows 版 ChatGPT 的本地 Codex 会话编辑器，用于定制续聊上下文。支持按项目或时间查找已归档会话。

## 下载与运行

下载 Windows x64 单文件程序后双击运行。需要 Microsoft Edge WebView2 运行时和 Microsoft Visual C++ x64 运行库。[下载程序](https://github.com/beijixiong258/chatgpt-zhushou/releases/tag/release)

程序默认读取 `%USERPROFILE%\.codex`。可通过 `CODEX_HOME` 指定 Codex 目录，也可通过 `CODEX_SQLITE_HOME` 或该目录下 `config.toml` 的 `sqlite_home` 指定 SQLite 数据库目录。

## 定制上下文

打开会话后可查看开场上下文，按原标题编辑或移除系统提示词段落，并逐项修改已记录的 AGENTS.md、主机技能和其他指令。可按 XML 标签编辑历史文本，删除单条消息、整轮问答，或成对移除工具调用及其结果。

完成技术选型后，可保留选定方案、决策依据和实施约束，移除已排除方案的讨论与相关工具记录。若同一条消息涉及多个方案，可编辑该消息以保留后续工作需要的信息。保存后继续原会话，可减少无关内容对后续回答的影响。

上下文压缩会将较早的会话内容归纳为供续聊使用的摘要。压缩前整理历史，可以减少无关讨论进入摘要。对于已有压缩点的会话，可编辑最近一次压缩之后的记录。

## 使用

对话助手只列出已归档会话。打开、保存和删除时均会检查归档状态。

1. 等待目标会话的当前回合或上下文压缩结束，再在 ChatGPT 中将其归档。
2. 点击对话助手中的「刷新列表」，从「已归档会话」打开目标会话。
3. 编辑所需内容，点击「保存修改」。
4. 保存成功后，点击「取消归档」，回到 ChatGPT 打开原会话。客户端续聊时可能重新注入当前指令。

「取消归档」使用本机的 Codex 命令行程序。若命令行程序不可用，也可在 ChatGPT 中取消归档。
在首页开启「批量选择」后，可对所选会话一起取消归档或删除。

保存时会检查会话写锁和历史版本。若会话仍被 ChatGPT 占用，等待其关闭后重试。若历史已经变化，重新打开会话并复核改动后再保存。

「加油白龙娘」按固定的 28 项清单，将当前会话首次提问前匹配的段落标记为待删除。清单包含 17 个系统提示词段落，以及桌面端上下文、权限说明、环境上下文、协作模式配置中的开发者指令、客户端时间上下文、主机技能、协作模式、工具、推荐插件、多智能体角色和多智能体模式。待删除项可逐项撤销。

删除整段会话时，点击「删除会话」并确认永久删除。批量删除同样会永久删除所选会话的本地历史。若其他分支仍引用该会话的历史，程序会取消删除。

## 构建

在 Windows x64 环境准备 Rust MSVC、Visual Studio C++ Build Tools、Windows SDK、Node.js、pnpm 10 和 PowerShell 7。Rust 和 pnpm 的版本以项目配置为准。

在项目根目录执行以下命令。

```powershell
pnpm install --frozen-lockfile
pnpm build
```

构建产物为 `outputs/chatgpt对话助手.exe`，内嵌前端、图标与许可声明。

开发时运行 `pnpm dev`。用 `pnpm typecheck` 检查 TypeScript，用 `pnpm test:unit` 运行 Rust 测试。

## 源码结构

- `src/desktop.rs` 创建原生 Windows 窗口并调度后台工作。`src/web.rs` 向 WebView2 提供内嵌页面，`src/bridge.rs` 分派会话命令。
- `src/items.rs` 与 `src/items/` 负责定位和编辑开场指令、对话与工具记录。
- `src/store.rs` 与 `src/store/` 负责会话文件、SQLite 索引、写锁和会话删除。
- `ui/` 包含 React 界面和桌面通信接口。`assets/` 存放首页配图、图标和第三方声明。
- `scripts/` 存放 Windows 发布脚本和依赖许可收集脚本。
