# Skills Manager Desktop

一个基于 Tauri 2、React 19 和 Rust 的 AI Agent Skill 桌面工作台。它把 Codex、Claude Code、Cursor、Gemini CLI、OpenCode、Windsurf 等工具的 Skill 管理、市场安装、预览审查、本地编辑、同步、备份、主题个性化和桌宠能力集中到一个桌面应用里。

本仓库是在 `xingkongliang/skills-manager` 基础上继续完善的桌面增强版本，重点把原本的 Skill 管理器扩展成可直接用于日常工作的 Skill Workbench。

## 核心能力

### Skill 工作台

- Skill 详情页支持完整文件树，能查看 `SKILL.md`、`README.md`、references、agents、配置文件等组成。
- 支持 Preview、Edit、Diff、Quality、History 多模式。
- 编辑器使用 CodeMirror，适合编辑 Markdown、YAML、JSON、TXT 等文本文件。
- 保存前先生成 diff，用户确认后再写入文件。
- 保存时校验 `original_hash`，如果文件在编辑期间被其他流程修改，会拒绝覆盖。
- 后端限制文件访问范围，只能读写当前 Skill 的 `central_path` 内部文件。
- 拒绝路径穿越、二进制文件和超大文件编辑。
- 保存成功后更新 content hash、`updated_at`、metadata、audit history，并触发相关同步状态刷新。

### 市场预览与安装审查

- 内置 skills.sh 市场预览，不再只能一键安装或跳转网页。
- 点击市场卡片会打开原生详情弹层。
- 市场详情支持文件树、主文档、文本文件预览、安装量、来源信息和风险提示。
- 安装前可设置本地名称、标签、是否加入当前 preset。
- 安装成功后可直接进入本地 Skill 工作台继续查看或编辑。
- 市场预览结果会尽量缓存，减少重复打开时的等待。

### 质量检查、历史与导出

- 支持检查缺失 `SKILL.md` / `README`、frontmatter 不完整、描述为空、缺失引用、疑似危险命令、超大文件和非文本资源。
- Quality 标签页按 error、warning、info 展示问题。
- 每条问题尽量提供文件路径和行号，方便定位。
- History 标签页展示安装、编辑、同步、更新、导出、恢复等操作记录。
- 编辑前保存快照，支持从历史中查看变更并做本地回滚。
- 支持将单个 Skill 导出为 `.zip`，便于分享、审查或备份。

### Agent 与工作区同步

- 支持全局工作区、项目工作区、关联工作区和 Library 入口。
- 支持 Codex、Claude Code、Cursor、Gemini CLI、OpenCode、Grok、Windsurf、GitHub Copilot 等 Agent。
- 支持按 Agent 启用/禁用 Skill。
- 支持 symlink 和 copy 两种同步方式。
- 支持自定义 Agent 路径和项目路径。
- 本地编辑 Skill 后可与原有同步逻辑联动，保持中央库和 Agent 工作区一致。

### 主题、壁纸和个性化

- 支持 `light`、`dark`、`system`、`sakura`、`anime`、`cyber`、`soft` 主题。
- 支持图片壁纸和 MP4/WebM/M4V 视频壁纸。
- 选择壁纸或视频时会导入到应用媒体目录，避免打包版因本地路径权限导致加载失败。
- 可设置背景透明度、模糊、暗化、面板透明度。
- 视频壁纸支持播放速度、音量和静音开关。
- 使用 Tauri asset protocol 和 CSP 白名单，保证打包后的桌面程序也能加载本地媒体。

### Live2D 桌宠

- 使用 `pixi-live2d-display` 集成 Live2D 桌宠。
- 内置 Hiyori、Mao、Wanko 示例模型。
- 支持配置多个桌宠。
- 每个桌宠支持启用开关、模型路径、位置、缩放、透明度、X/Y 偏移。
- 支持放在侧边栏、右下角、左下角、右上角等位置。

### 备份与多设备同步

- 可连接 GitHub 或任意 Git remote 做 Skill 库备份。
- 支持自动提交、推送、拉取和合并。
- 支持快照版本和恢复。
- 支持冲突检测与处理。
- 备份内容包含 Skill 文件、标签、preset、Agent 开关等。
- API key、token、代理设置等本机私密配置不会进入 Git。

## 技术栈

| 层级 | 技术 |
| --- | --- |
| 桌面框架 | Tauri 2 |
| 前端 | React 19、TypeScript、Vite |
| 样式 | Tailwind CSS |
| 后端 | Rust |
| 数据库 | SQLite / rusqlite |
| 编辑器 | CodeMirror |
| Diff | diff |
| Live2D | pixi.js、pixi-live2d-display、live2dcubismcore |
| 国际化 | i18next、react-i18next |

## 项目结构

```text
.
├── public/
│   ├── live2d/              # 内置 Live2D 模型
│   └── vendor/              # Live2D Cubism Core 等静态资源
├── scripts/                 # Windows 开发、检查、打包脚本
├── src/
│   ├── components/          # UI 组件、Skill 工作台、市场预览、桌宠
│   ├── context/             # 全局上下文
│   ├── hooks/               # 主题、桌宠、状态相关 hooks
│   ├── lib/                 # Tauri API 封装、工具函数
│   └── views/               # 页面视图
├── src-tauri/
│   ├── src/
│   │   ├── commands/        # Tauri commands
│   │   └── core/            # Skill、同步、Git 备份核心逻辑
│   ├── tauri.conf.json
│   └── tauri.dev.conf.json
├── package.json
└── README.md
```

## 本地开发

### 环境要求

- Windows 10/11、macOS 或 Linux
- Node.js 20.19+ 或 22.12+
- Rust toolchain
- Tauri 2 所需系统依赖
- Windows 打包需要 NSIS / WebView2 环境

### 安装依赖

```powershell
npm install
```

### 启动开发版

```powershell
npm run tauri:dev
```

Windows 下也可以使用项目脚本：

```powershell
npm run tauri:dev:win
```

### 前端构建

```powershell
npm run build
```

### 代码检查

```powershell
npm run lint
npm run cargo:check:win
```

### 打包 Windows 桌面程序

```powershell
npm run tauri:build:win:local
```

打包产物默认在：

```text
src-tauri/target/release/bundle/nsis/skills-manager_1.28.0_x64-setup.exe
```

## 常用脚本

| 命令 | 说明 |
| --- | --- |
| `npm run dev` | 启动 Vite |
| `npm run build` | TypeScript + Vite 构建 |
| `npm run lint` | ESLint 检查 |
| `npm run tauri:dev` | Tauri 开发模式 |
| `npm run tauri:dev:win` | Windows Tauri 开发脚本 |
| `npm run cargo:check:win` | Windows Rust 检查脚本 |
| `npm run tauri:build:win:local` | Windows 本地 NSIS 打包 |
| `npm run cli` | 运行 Rust CLI |
| `npm run cli:build` | 构建 CLI |
| `npm run cli:install` | 安装 CLI 到本机 PATH |

## 使用流程

1. 打开应用后进入 Skill Library。
2. 从本地目录、Git 仓库、压缩包或 skills.sh 市场安装 Skill。
3. 点击 Skill 打开工作台，查看文件树、预览文档、编辑文件。
4. 编辑后进入 Diff 标签页确认变更，再保存。
5. 在 Quality 标签页检查 Skill 是否存在风险或结构问题。
6. 在 Agent / Workspace 页面把 Skill 同步到 Codex、Cursor、Claude Code 等目标工具。
7. 在 Settings 里配置主题、壁纸、视频背景、Live2D 桌宠和 Agent 路径。
8. 在 Backup 页面配置 Git 备份，实现版本历史和多设备同步。

## Live2D 模型说明

内置模型位于：

```text
public/live2d/Hiyori/Hiyori.model3.json
public/live2d/Mao/Mao.model3.json
public/live2d/Wanko/Wanko.model3.json
```

应用中默认配置路径为：

```text
/live2d/Hiyori/Hiyori.model3.json
/live2d/Mao/Mao.model3.json
/live2d/Wanko/Wanko.model3.json
```

如果导入自定义模型，请选择 `model3.json` 文件，并确保同目录下存在对应的 `.moc3`、纹理、动作、物理文件等资源。

## 本地媒体加载说明

桌面打包环境不能像普通网页一样随意读取磁盘文件。本项目为壁纸、视频和 Live2D 做了以下处理：

- `tauri.conf.json` 开启 `assetProtocol`。
- CSP 放行 `asset:`、`http://asset.localhost`、`https://asset.localhost`、`img-src`、`media-src` 和 `connect-src`。
- 选择壁纸或视频时复制到应用数据目录的 `appearance-media` 中。
- 前端通过 `convertFileSrc` 把本地文件路径转换为 Tauri 可访问 URL。

因此，打包后的应用也可以稳定加载本地图片、视频和模型资源。

## 安全边界

- Skill 文件编辑只允许发生在该 Skill 的中央库路径内。
- 路径穿越、绝对路径逃逸、跨 Skill 访问都会被拒绝。
- 二进制和超大文件不会进入文本编辑器。
- 保存时使用 hash 做冲突检测。
- Git 备份不会上传本机 token、API key 和代理密码。

## 验证记录

本地验证使用以下命令：

```powershell
npm run lint
npm run build
npm run cargo:check:win
npm run tauri:build:win:local
```

已知提示：

- 当前环境 Node.js 为 20.15.0，Vite 建议升级到 20.19+ 或 22.12+。
- 前端主 bundle 较大，后续可考虑继续拆分 `pixi`、`CodeMirror` 和市场预览模块。

## License

MIT
