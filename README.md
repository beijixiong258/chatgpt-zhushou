# chatgpt对话助手

用于 Windows 上 ChatGPT 客户端的本地 Codex 会话。支持按项目或时间查找会话、按 XML 标签编辑历史文本、删除消息或整轮问答、清除配对的工具调用，以及将整段会话移入桌面的智能体回收站。打开会话即可查看开场上下文。展开「系统提示词」可按原标题编辑或移除段落。AGENTS.md、主机技能和其他已记录的指令也可以逐项修改。

## 运行

「加油白龙娘」按固定的 18 项清单，将当前会话首次提问前匹配的段落标记为待删除。清单包含 13 个系统提示词段落，以及桌面端上下文、权限说明、环境上下文、协作模式配置中的开发者指令和客户端时间上下文。可以逐项撤销，点「保存修改」后才写回。

双击 `chatgpt对话助手.exe`。Windows 需要 Microsoft Edge WebView2 运行时。首次启动从本机 Codex 目录读取会话，支持 `CODEX_HOME` 及单独配置的 SQLite 目录。保存前目标会话需要释放写锁；完整退出 ChatGPT 后保存，再用原会话继续即可。

编辑作用于本机会话历史，客户端续聊时可以重新注入当前指令。遇到跨越上下文压缩点的改动，可编辑最近一次压缩之后的记录。整会话删除时保留其他分支仍引用的文件，存在依赖时取消本次删除。

## 构建

在具备 Rust MSVC、Visual Studio C++ Build Tools、WebView2、Node.js、pnpm 和 PowerShell 7 的 Windows 机器上，从项目根目录执行

```powershell
pnpm install --frozen-lockfile
pnpm build
```

成品位于 `src-tauri\target\release\chatgpt对话助手.exe`。运行 `pnpm typecheck` 检查前端，运行 `pnpm test:unit` 验证 Rust 编辑逻辑。

## 源码结构

- `src/HistoryEditor.tsx` 管理会话列表、当前草稿和编辑界面。`src/history.ts` 定义前后端交换的数据和四个会话命令。
- `src-tauri/src/lib.rs` 注册桌面命令，并把文件与数据库操作调度到后台线程。
- `src-tauri/src/items.rs` 从已读取的 JSON 记录中定位消息与标签，并在内存中应用编辑。`items/prompts.rs` 定位开场指令、标题和对应副本。`items/tests.rs` 存放编辑逻辑的单元测试。
- `src-tauri/src/store.rs` 负责会话文件、写锁和 SQLite 索引。`store/recycle.rs` 管理整会话回收及中断恢复。

基于 CC Switch 的 Tauri 桌面项目整理，许可证见 LICENSE。
