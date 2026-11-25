# chatgpt对话助手

用于 Windows 上 ChatGPT 客户端的本地 Codex 会话。支持按项目或时间查找会话、按 XML 标签编辑历史文本、删除消息或整轮问答、清除配对的工具调用，以及将整段会话移入桌面的智能体回收站。打开会话即可查看开场上下文。展开「系统提示词」可按原标题编辑或移除段落。AGENTS.md、主机技能和其他已记录的指令也可以逐项修改。

## 运行

「加油白龙娘」按固定的 18 项清单，将当前会话首次提问前匹配的段落标记为待删除。清单包含 13 个系统提示词段落，以及桌面端上下文、权限说明、环境上下文、协作模式配置中的开发者指令和客户端时间上下文。可以逐项撤销，点「保存修改」后才写回。

双击 `chatgpt对话助手.exe`。Windows 需要 Microsoft Edge WebView2 运行时。首次启动从本机 Codex 目录读取会话，支持 `CODEX_HOME` 及单独配置的 SQLite 目录。保存前目标会话需要释放写锁；完整退出 ChatGPT 后保存，再用原会话继续即可。

编辑作用于本机会话历史，客户端续聊时可以重新注入当前指令。遇到跨越上下文压缩点的改动，可编辑最近一次压缩之后的记录。整会话删除时保留其他分支仍引用的文件，存在依赖时取消本次删除。

## 构建

准备 Windows x64、Rust MSVC、Visual Studio C++ Build Tools 与 Windows SDK、Node.js、pnpm 10 和 PowerShell 7。在项目根目录执行

```powershell
pnpm install --frozen-lockfile
pnpm build
```

单文件程序位于 `outputs/chatgpt对话助手.exe`。它内嵌前端、图标与许可声明。目标电脑需要 Microsoft Edge WebView2 运行时和 Microsoft Visual C++ x64 运行库。

开发时运行 `pnpm dev`。用 `pnpm typecheck` 检查 TypeScript，用 `pnpm test:unit` 运行 Rust 测试。

## 源码结构

- `src/desktop.rs` 创建原生 Windows 窗口并调度后台工作。`src/web.rs` 向 WebView2 提供内嵌页面，`src/bridge.rs` 分派会话命令。
- `src/items.rs` 与 `src/items/` 负责定位和编辑开场指令、对话与工具记录。
- `src/store.rs` 与 `src/store/` 负责会话文件、SQLite 索引、写锁和回收操作。
- `ui/` 包含 React 界面和桌面通信接口。`assets/` 存放首页配图、图标和第三方声明。
- `scripts/` 存放 Windows 发布脚本和依赖许可收集脚本。
