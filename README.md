# LuZhi / 录智

录智是一个基于 Tauri 2 的桌面录屏 MVP：专注于“录屏 + 录后自动美化 + 一键导出”的单点工作流。

当前仓库处于 MVP 阶段：macOS 录制主链路优先打穿，Windows MVP 已进入实现路径，WindowsRecordingService 已接入 WGC/WASAPI/cpal/FFmpeg 管线。

## 当前状态

| 平台     | 状态                           | 说明                                                                                        |
| -------- | ------------------------------ | ------------------------------------------------------------------------------------------- |
| macOS    | 可开发、可运行 MVP             | 使用 ScreenCaptureKit + cpal + FFmpeg feature 跑通全屏录制、预览、美化和导出                |
| Windows  | MVP 开发中                     | 应用可启动；录制主链路按 Windows Graphics Capture / WASAPI / cpal / FFmpeg 逐步验收         |
| Web 前端 | 可独立开发和测试               | `npm run dev` 启动 Vite，适合 UI 调试                                                       |

## 功能概览

### 已覆盖的 MVP 能力

- 全屏录制工作流：开始、暂停、恢复、停止。
- 画面参数入口：720p、1080p、4K 实验性选项；MVP 主验收目标为 1080p。
- 音频入口：系统音频、麦克风、麦克风设备选择、麦克风电平展示、基础高通降噪开关。
- macOS 权限检测：屏幕录制、麦克风、辅助功能状态展示。
- 录后预览：内置视频预览、播放/暂停、前后跳转、进度条、音量、静音、全屏。
- 光标美化：光标轨迹平滑、点击效果、光标形态元数据、预览 overlay。
- 自动裁剪：基于音频静音和画面活动的空白段检测，支持低/中/高灵敏度。
- 导出预设：
  - Bilibili / YouTube：`1920x1080`
  - 抖音：`1080x1920`
  - 小红书：`1080x1080`
- 历史录制库：录制列表、重新进入预览、导入本应用录制、删除。
- 本地试用状态：14 天本地试用状态接口；服务端激活协议尚未接入。

### 暂未完成或受限

- 窗口录制、区域录制：UI 已展示入口，后端当前会提示“正在开发中”。
- Windows 原生录制：DXGI Desktop Duplication 和 WASAPI loopback 仍是占位模块。
- 完整激活码服务端协议：当前只保留接口边界。
- 平台发布 API、字幕、摘要、知识库、模板系统、团队协作：MVP 明确不做。
- 4K 与 60fps：作为架构和 UI 预留能力，MVP 不作为稳定硬验收。

## 技术栈

- 桌面框架：Tauri 2
- 前端：React 19 + TypeScript + Vite 7 + Tailwind CSS 4
- UI 组件：shadcn/ui 风格组件、Radix UI、lucide-react、Framer Motion
- Rust 原生层：
  - macOS：ScreenCaptureKit、CoreMedia/CoreVideo、CoreAudio/cpal、AppKit/Objective-C FFI
  - Windows：DXGI / WASAPI 模块预留
  - 媒体处理：FFmpeg C API binding (`ffmpeg-next`，通过 Cargo feature 启用)
- 测试：Vitest + Testing Library、Rust unit/integration tests

## 架构原则

录智采用三层架构：

```text
React UI
  - 只负责展示、交互、轻量状态
  - 通过 Tauri invoke/event 与 Rust 通信
  - 不接触视频帧和音频流

Rust App Logic
  - 状态机、录制服务、导出服务、授权服务、权限服务
  - 管理任务生命周期、错误归一化和应用事件

Native / Media Modules
  - ScreenCaptureKit、cpal、FFmpeg、光标元数据、音频混音、裁剪与导出
  - 捕获线程不得被 UI、美化、裁剪或导出阻塞
```

关键红线：音视频帧流不得经过前端 JS 层。前端只接收状态、时长、进度、错误、文件路径等轻量数据。

## 目录结构

```text
.
├── src/                         # React 前端
│   ├── components/              # 录制面板、预览页、历史侧栏等 UI
│   ├── components/ui/           # 本地 shadcn/ui 风格基础组件
│   ├── lib/tauri.ts             # 前端 Tauri command/event 封装
│   └── App.tsx                  # 主状态编排
├── src-tauri/                   # Tauri / Rust 原生层
│   ├── src/app/                 # 应用服务、状态机、权限、录制库
│   ├── src/core/                # 捕获、帧、时间线、配置等核心抽象
│   ├── src/media/               # FFmpeg、音频、光标、裁剪、导出
│   ├── src/platform/macos/      # macOS ScreenCaptureKit / cpal / cursor
│   └── src/platform/windows/    # Windows DXGI / WASAPI 预留模块
├── docs/                        # PRD、架构、计划、review 记录
├── reference/                   # UI 和任务参考材料
├── tests/                       # 人工/阶段自测清单
├── BUG.md                       # 已知 bug、修复记录和预防规则
├── HANDOFF.md                   # 项目进度交接文档
└── package.json                 # 前端与 Tauri 命令入口
```

## 本地开发环境准备

### 通用要求

所有平台都需要：

- Git
- Node.js：`>=20.19.0`，或 `>=22.12.0`，或 `>=24.0.0`
- npm：建议使用随 Node.js 附带的版本
- Rust stable toolchain：`rustup` + `cargo`
- Tauri 2 系统依赖：参考官方文档 <https://v2.tauri.app/start/prerequisites/>

安装项目依赖：

```bash
npm install
```

检查版本：

```bash
node --version
npm --version
rustc --version
cargo --version
```

### macOS 环境准备

#### 1. 系统版本

录智的 macOS 录屏主链路依赖 ScreenCaptureKit。项目文档中的最低运行版本为 macOS 12.3 Monterey。

建议开发环境：

- macOS 12.3 或更新版本
- Xcode Command Line Tools
- Homebrew

#### 2. 安装 Xcode Command Line Tools

```bash
xcode-select --install
```

如果已经安装过，可检查：

```bash
xcode-select -p
```

#### 3. 安装 Homebrew

Homebrew 官方安装方式见 <https://brew.sh/>。

安装后确认：

```bash
brew --version
```

#### 4. 安装 Node.js

可以用 `nvm`、`fnm`、Volta 或 Homebrew。示例使用 Homebrew：

```bash
brew install node
node --version
```

如果你使用版本管理工具，请选择满足本仓库 Vite 依赖要求的 Node.js 版本：`20.19+`、`22.12+` 或 `24+`。

#### 5. 安装 Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable
rustc --version
```

如果当前终端找不到 `cargo`，重启终端或重新加载 shell 配置。

#### 6. 安装 FFmpeg 开发库

要得到可播放录制文件和真实导出，需要启用 Cargo `ffmpeg` feature，并安装 FFmpeg development libraries：

```bash
brew install ffmpeg pkg-config
```

如果构建时提示找不到 FFmpeg 的 pkg-config 文件，可临时指定：

```bash
export PKG_CONFIG_PATH="$(brew --prefix ffmpeg)/lib/pkgconfig:$PKG_CONFIG_PATH"
```

#### 7. 克隆并安装依赖

```bash
git clone https://github.com/1394225561/LuZhi.git
cd LuZhi
npm install
```

#### 8. 启动 macOS 原生开发版

推荐使用 FFmpeg feature 启动，这样录制和导出链路才会写出真实视频文件：

```bash
npm run tauri:dev:ffmpeg
```

普通 Tauri dev 命令也可启动应用壳，但非 FFmpeg 构建使用计数 writer，适合轻量联调，不适合验证可播放录制/导出：

```bash
npm run tauri:dev
```

#### 9. 授权系统权限

首次录制时，macOS 可能需要授权：

- 屏幕录制：系统设置 → 隐私与安全性 → 屏幕录制
- 麦克风：系统设置 → 隐私与安全性 → 麦克风

授权后通常需要重启应用。若权限状态显示异常，先关闭开发中的 Tauri 应用，再重新运行 `npm run tauri:dev:ffmpeg`。

### Windows 环境准备

当前 Windows 原生应用尚不能完整构建运行：`src-tauri/src/lib.rs` 对非 macOS 目标有编译期保护，Windows DXGI/WASAPI 模块也仍是占位实现。

下面步骤用于准备 Windows 开发环境、运行前端、阅读/补齐 Windows 原生模块。等 WindowsRecordingService 接入后，可在同一环境继续启用完整 Tauri 运行。

#### 1. 建议系统

- Windows 10 或 Windows 11
- PowerShell 7 或 Windows Terminal
- Microsoft Edge WebView2 Runtime
- Visual Studio Build Tools 2022

#### 2. 安装 Visual Studio Build Tools

安装 Visual Studio Build Tools 2022：<https://visualstudio.microsoft.com/visual-cpp-build-tools/>

请勾选：

- Desktop development with C++
- MSVC v143 build tools
- Windows 10/11 SDK
- CMake tools for Windows

安装后打开新的 PowerShell，并确认：

```powershell
cl
```

如果提示找不到 `cl`，可以使用 “Developer PowerShell for VS 2022”，或重新检查 Build Tools 安装项。

#### 3. 安装 WebView2 Runtime

Tauri 在 Windows 上依赖系统 WebView2。通常 Windows 11 已内置；如果缺失，可安装 Evergreen Runtime：

<https://developer.microsoft.com/microsoft-edge/webview2/>

#### 4. 安装 Node.js

推荐安装 Node.js LTS，并确保版本满足 `20.19+`、`22.12+` 或 `24+`。

可用 winget：

```powershell
winget install OpenJS.NodeJS.LTS
node --version
npm --version
```

#### 5. 安装 Rust MSVC toolchain

```powershell
winget install Rustlang.Rustup
rustup default stable-msvc
rustc --version
cargo --version
```

如果当前 PowerShell 找不到 `rustup` 或 `cargo`，重新打开终端。

#### 6. 可选：准备 FFmpeg 开发库

Windows 原生录制尚未接入，因此现在不要求 FFmpeg dev libs 才能做前端开发。等 Windows 端启用 `ffmpeg` feature 后，建议使用 vcpkg 或项目约定的统一方式安装 FFmpeg 开发包，并配置 `PKG_CONFIG_PATH` / `VCPKG_ROOT` 等环境变量。

请不要把本地 FFmpeg DLL、构建产物或绝对路径提交到仓库。

#### 7. 克隆并安装依赖

```powershell
git clone https://github.com/1394225561/LuZhi.git
cd LuZhi
npm install
```

#### 8. Windows 当前可运行的开发入口

前端 UI：

```powershell
npm run dev
```

前端测试：

```powershell
npm test -- --run
```

完整 Tauri 原生 Windows 运行当前会被编译期保护阻止，这是预期状态：

```powershell
npm run tauri:dev
```

看到非 macOS 构建相关错误时，不要先改依赖版本；应从 WindowsRecordingService、DXGI/WASAPI 实现和 `src-tauri/src/lib.rs` 的平台接线开始。

## 开发命令

### 前端

启动 Vite：

```bash
npm run dev
```

构建前端：

```bash
npm run build
```

预览前端构建产物：

```bash
npm run preview
```

运行前端测试：

```bash
npm test -- --run
```

### Tauri

macOS 原生开发：

```bash
npm run tauri:dev
```

macOS 原生开发，启用 FFmpeg：

```bash
npm run tauri:dev:ffmpeg
```

打包 Tauri 应用：

```bash
npm run tauri -- build
```

启用 FFmpeg 打包：

```bash
npm run tauri -- build --features ffmpeg
```

### Rust

运行 Rust 测试：

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

运行 FFmpeg feature 测试：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
```

格式检查：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

Clippy：

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --lib
```

## 推荐开发流程

1. 先阅读 `HANDOFF.md`，确认当前阶段、最近修改和人工验证建议。
2. 阅读 `BUG.md`，尤其是“预防规则”，避免重复踩已修复问题。
3. UI 相关修改先对齐 `reference/ui/ui_spec.md`。
4. 小改动优先运行目标测试；涉及共享行为、录制链路或导出链路时，运行更完整的前端和 Rust 回归。
5. macOS 录制/导出类改动需要人工验证真实 Tauri 应用，尤其是系统权限、光标、音频、导出文件可播放性。

## 测试策略

本仓库同时包含自动化测试和人工自测清单：

- `src/**/*.test.tsx`：前端组件和状态行为测试。
- `src-tauri/src/**`：Rust 单元测试。
- `src-tauri/tests/`：Rust 集成测试。
- `tests/*.md`：阶段验收和人工验证清单。

常用回归组合：

```bash
npm test -- --run
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
```

涉及真实编码、导出、光标 overlay 或音频 contract 时，补充：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
npm run tauri:dev:ffmpeg
```

## 数据与产物

- Tauri app data dir：用于录制库索引和授权试用状态。
- 临时目录 `luzhi-recordings/`：用于部分录制、中间元数据、导出和测试产物。
- 本地生成的 `node_modules/`、`dist/`、`src-tauri/target/` 不应提交。
- 请勿提交用户录制视频、麦克风音频、系统日志、激活码或 `.env` 文件。

## 常见问题

### `npm run tauri:dev:ffmpeg` 找不到 FFmpeg

确认安装：

```bash
brew install ffmpeg pkg-config
```

确认 pkg-config 能看到 FFmpeg：

```bash
pkg-config --libs libavformat
pkg-config --libs libavcodec
```

如果找不到，尝试：

```bash
export PKG_CONFIG_PATH="$(brew --prefix ffmpeg)/lib/pkgconfig:$PKG_CONFIG_PATH"
```

### macOS 提示没有屏幕录制权限

到系统设置授权：

```text
系统设置 → 隐私与安全性 → 屏幕录制
```

授权后重启应用。开发模式下，可能需要退出当前 Tauri 窗口并重新运行命令。

### 录制后没有可播放文件

确认使用的是 FFmpeg feature：

```bash
npm run tauri:dev:ffmpeg
```

普通 `npm run tauri:dev` 主要用于应用壳和非编码路径联调，不适合验证最终视频文件。

### Windows 上 Tauri 编译失败

这是当前 MVP 的已知状态。Windows 端需要先实现并接入 WindowsRecordingService，再移除非 macOS 编译保护。现在可以先做前端开发、阅读 Windows 预留模块或补齐底层设计。

## 贡献说明

欢迎围绕 MVP 主线贡献：

- macOS 录制稳定性、音频同步、FFmpeg 导出质量。
- Windows DXGI/WASAPI 实现与平台接线。
- 光标美化、空白裁剪、历史录制库的 bug 修复。
- 测试覆盖、人工验证清单和文档完善。

贡献前请注意：

- 不要直接升级 `Cargo.toml` 核心依赖版本，除非已经单独说明原因并完成验证。
- 跨平台底层 API、FFI、线程和资源释放相关代码必须经过严格人工 review。
- 不要新增产品路线图外的功能。
- 不要提交录制产物、隐私数据、密钥或生产环境配置。

## 参考文档

- `HANDOFF.md`：项目进度和最近工作记录。
- `BUG.md`：已知 bug、修复记录和预防规则。
- `docs/architecture/project-architecture-and-overall-planning.md`：系统架构与 MVP 计划。
- `docs/PRD/LuZhi_PRD_final_version.md`：产品方向与 MVP 功能边界。
- `docs/platform-diff/macos-compatibility.md`：macOS 兼容性要求。
- `reference/ui/ui_spec.md`：UI 设计规范。
- Tauri 2 prerequisites：<https://v2.tauri.app/start/prerequisites/>

## 许可证

本仓库当前尚未包含 `LICENSE` 文件。正式开源前需补充明确许可证，例如 MIT、Apache-2.0 或双许可证方案。
