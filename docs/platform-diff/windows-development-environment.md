# Windows 开发环境准备

> 最后更新：2026-06-19
>
> 适用阶段：Windows MVP 开发。Windows 原生应用已进入 MVP 实现路径：`npm run tauri:dev` 应能启动应用壳；录制能力按 Windows Graphics Capture、WASAPI、cpal 麦克风和 FFmpeg feature 的接入状态逐步验证。

## 1. 当前项目边界

LuZhi 当前技术栈：

| 层级 | 技术 |
|------|------|
| 桌面框架 | Tauri 2 |
| 前端 | React 19 + TypeScript + Vite 7 + Tailwind CSS 4 |
| Rust 原生层 | macOS ScreenCaptureKit；Windows 计划使用 DXGI / WASAPI |
| 媒体处理 | `ffmpeg-next`，通过 Cargo `ffmpeg` feature 启用 |

当前 Windows 侧状态：

- `src-tauri/src/lib.rs` 已移除 macOS 编译期保护，Windows 构建路径已开放。
- `WindowsRecordingService` 实现了 `PlatformRecordingService` trait，接入 WGC/WASAPI/cpal/FFmpeg 管线。
- `src-tauri/src/platform/windows/` 包含以下已实现模块：
  - `graphics_capture.rs`：Windows Graphics Capture 帧辅助工具和全屏/窗口适配器骨架
  - `wasapi_loopback.rs`：WASAPI 回环音频捕获生命周期管理
  - `window_capture.rs`：基于 EnumWindows 的窗口枚举
  - `cursor_source.rs`：Windows 光标位置和按键状态采集
  - `audio_device.rs`：音频格式转换辅助工具
- Windows 窗口录制采用 Windows Graphics Capture，不以 DXGI 桌面帧裁剪作为正式窗口录制方案。若窗口最小化、关闭、受保护或 API 不支持，应用应给出中文错误，不生成假成功录制。

---

## 2. 基础系统要求

### 2.1 Windows 版本

检查：

```powershell
winver
```

建议：

| 项目 | 建议 |
|------|------|
| 系统 | Windows 10 最新补丁或 Windows 11 |
| 终端 | PowerShell 7 或 Windows Terminal |
| 权限 | 常规开发不需要管理员终端；安装工具链时可能需要管理员权限 |

不通过时：

1. 先通过 Windows Update 更新系统。
2. 不建议在过旧 Windows 上开发 DXGI/WASAPI 链路。

---

## 3. Git

检查：

```powershell
git --version
```

不通过时安装：

```powershell
winget install --id Git.Git -e
```

安装后关闭并重新打开终端，复查：

```powershell
git --version
```

---

## 4. Node.js 与 npm

项目使用 Vite 7，Node.js 版本需要满足以下任一条件：

- `>=20.19.0`
- `>=22.12.0`
- `>=24.0.0`

检查：

```powershell
node --version
npm --version
```

不通过时，推荐安装 Node.js LTS：

```powershell
winget install --id OpenJS.NodeJS.LTS -e
```

如果需要精确管理版本，推荐 Volta：

```powershell
winget install --id Volta.Volta -e
volta install node@22
```

安装后关闭并重新打开终端，复查：

```powershell
node --version
npm --version
```

---

## 5. Visual Studio Build Tools 2022

Tauri Windows 原生构建、Rust MSVC toolchain、`ffmpeg-next` 链接都依赖 MSVC 构建工具。

### 5.1 检查 MSVC 是否可用

普通 PowerShell 中直接运行：

```powershell
cl
```

可能会失败，因为普通终端默认不加载 MSVC 环境变量。更可靠的检查方式是：

```powershell
cmd /c '"C:\Program Files\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat" && cl'
```

通过时应看到类似：

```text
Microsoft (R) C/C++ Optimizing Compiler Version ...
```

### 5.2 检查 Build Tools 实例

```powershell
& "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" -products * -format table
```

通过时应能看到 `BuildTools` 实例。

继续检查 MSVC 目录：

```powershell
Get-ChildItem "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC"
```

通过时应看到版本目录，例如 `14.xx.xxxxx`。

### 5.3 不通过时安装

安装 Build Tools：

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools -e
```

注意：该命令有时只安装 Visual Studio Installer 或 Build Tools 外壳，不一定自动安装 C++ workload。如果安装过程没有出现可勾选界面，安装后仍找不到 `cl.exe`，需要补装组件。

### 5.4 补装 C++ workload

推荐图形界面方式：

```powershell
Start-Process "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vs_installer.exe"
```

在 Visual Studio Installer 中：

1. 找到 `Build Tools 2022`。
2. 点击 `Modify / 修改`。
3. 勾选 `Desktop development with C++`。
4. 右侧确认至少包含：
   - MSVC v143 build tools
   - Windows 10 SDK 或 Windows 11 SDK
   - C++ CMake tools for Windows
5. 点击安装或修改。

命令行补装方式：

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --norestart"
```

补装后复查：

```powershell
& "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" -products * -format table
cmd /c '"C:\Program Files\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat" && cl'
```

### 5.5 常见误判

| 现象 | 判断 |
|------|------|
| 普通 PowerShell 中 `cl` 不存在 | 可能只是没加载 VS 环境变量 |
| `VsDevCmd.bat && cl` 也失败 | MSVC 组件大概率未安装 |
| `vswhere` 返回 `[]` | Build Tools 实例未完整安装 |
| `VC\Tools\MSVC` 目录不存在 | C++ workload 未安装 |

---

## 6. WebView2 Runtime

Tauri Windows 运行依赖 Microsoft Edge WebView2 Runtime。Windows 11 通常已内置，但建议显式检查。

检查：

```powershell
Get-Item 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction SilentlyContinue
```

不通过时安装：

```powershell
winget install --id Microsoft.EdgeWebView2Runtime -e
```

也可以从 Microsoft 官方页面安装 Evergreen Runtime：

<https://developer.microsoft.com/microsoft-edge/webview2/>

---

## 7. Rust MSVC Toolchain

检查：

```powershell
rustup --version
rustc --version
cargo --version
rustup show
```

重点确认 active toolchain 或 default host 包含：

```text
x86_64-pc-windows-msvc
```

不通过时安装：

```powershell
winget install --id Rustlang.Rustup -e
```

安装后关闭并重新打开终端：

```powershell
rustup default stable-msvc
rustup update
rustup show
rustc --version
cargo --version
```

如果看到 `x86_64-pc-windows-gnu`，切换到 MSVC：

```powershell
rustup default stable-msvc
```

---

## 8. 项目依赖安装与前端验证

进入项目根目录：

```powershell
cd D:\1_code-space\workspace_github\LuZhi
```

安装 npm 依赖：

```powershell
npm install
```

如果遇到 npm 网络问题，可临时切换 registry：

```powershell
npm config set registry https://registry.npmmirror.com
npm install
```

验证前端测试：

```powershell
npm test -- --run
```

验证前端构建：

```powershell
npm run build
```

---

## 9. Tauri CLI

本项目通过本地依赖提供 Tauri CLI，不建议优先安装全局 CLI，避免版本不一致。

检查：

```powershell
npx tauri --version
```

或：

```powershell
npm run tauri -- --version
```

不通过时：

```powershell
npm install
```

然后复查。

---

## 10. Windows 当前可用开发入口

当前可以稳定使用：

```powershell
npm run dev
npm test -- --run
npm run build
```

当前不应作为环境验收硬指标：

```powershell
npm run tauri:dev
npm run tauri:dev:ffmpeg
cargo check --manifest-path src-tauri/Cargo.toml
```

原因是 WindowsRecordingService 尚未接入，源码当前会在非 macOS 构建上触发编译期保护。

---

## 11. FFmpeg 开发环境

FFmpeg 环境需要区分两层：

| 层级 | 用途 | 是否足够支持 Rust `ffmpeg-next` |
|------|------|--------------------------------|
| `ffmpeg.exe` / `ffprobe.exe` 命令行工具 | 手工检查、转码、探测媒体文件 | 否 |
| FFmpeg development libraries | Rust 编译链接所需的 headers、`.lib`、DLL | 是 |

项目使用 `ffmpeg-next`，它绑定的是 FFmpeg C API，不是直接调用 `ffmpeg.exe`。因此，仅让 `ffmpeg -version` 通过，并不代表 `cargo test --features ffmpeg` 一定能通过。

### 11.1 检查 FFmpeg CLI

```powershell
ffmpeg -version
ffprobe -version
```

如果提示：

```text
ffmpeg: The term 'ffmpeg' is not recognized
```

说明 `ffmpeg.exe` 不在 PATH。

### 11.2 仅安装 CLI 的方式

可以安装：

```powershell
winget install --id Gyan.FFmpeg -e
```

安装后关闭并重新打开终端：

```powershell
ffmpeg -version
ffprobe -version
```

注意：这种方式通常只能解决 CLI，不保证包含 Rust 编译需要的 `include/` 和 `.lib`。

如果本机只有 LuZhi 一个 FFmpeg 使用方，不建议优先采用 CLI-only 安装方式。更推荐直接配置一套完整 shared dev 包，同时提供 CLI 和 Rust 链接所需文件。

如果本机还有其他项目需要 `ffmpeg.exe` / `ffprobe.exe`，也推荐让这些项目复用同一套完整 shared dev 包，避免 PATH 中出现多个 FFmpeg 版本。

### 11.3 Rust `ffmpeg-next` 推荐准备方式

`ffmpeg-next` 在 Windows/MSVC 下通常需要：

- LLVM / libclang，供 `bindgen` 使用。
- FFmpeg headers。
- FFmpeg import libraries。
- FFmpeg runtime DLL。
- 环境变量 `FFMPEG_DIR`。
- 运行时 PATH 包含 FFmpeg `bin`。

推荐最终目录：

```text
C:\dev\toolchains\ffmpeg\7.1
```

这套目录同时承担两层职责：

| 使用方 | 使用内容 |
|--------|----------|
| 其他项目 | `C:\dev\toolchains\ffmpeg\7.1\bin\ffmpeg.exe` 和 `ffprobe.exe` |
| LuZhi / Rust | `FFMPEG_DIR` 下的 `include/`、`lib/`、`bin/` |

#### 11.3.1 安装 LLVM / libclang

安装方式一：使用 winget：

```powershell
winget install --id LLVM.LLVM -e
```

安装方式二：手动下载安装包：

1. 打开 <https://github.com/llvm/llvm-project/releases>。
2. 下载 Windows x64 installer，例如 `LLVM-xx.x.x-win64.exe`。
3. 安装到默认路径 `C:\Program Files\LLVM`。

关闭并重新打开终端，检查：

```powershell
clang --version
Test-Path "C:\Program Files\LLVM\bin\libclang.dll"
```

如果 `C:\Program Files\LLVM\bin\clang.exe` 存在，但 `clang --version` 仍报找不到命令，说明 LLVM `bin` 未加入 PATH。`LIBCLANG_PATH` 只告诉 `bindgen` 去哪里找 `libclang.dll`，不会自动让 `clang.exe` 进入命令行 PATH。

设置 `LIBCLANG_PATH`：

```powershell
setx LIBCLANG_PATH "C:\Program Files\LLVM\bin"
```

可选但推荐：把 LLVM `bin` 加入用户 PATH，方便后续人工检查：

```powershell
$llvmBin = "C:\Program Files\LLVM\bin"
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (($userPath -split ";") -notcontains $llvmBin) {
  [Environment]::SetEnvironmentVariable("Path", ($userPath.TrimEnd(";") + ";" + $llvmBin), "User")
}
```

关闭所有 PowerShell / VS Code 终端，重新打开后复查：

```powershell
clang --version
echo $env:LIBCLANG_PATH
Test-Path "$env:LIBCLANG_PATH\libclang.dll"
```

#### 11.3.2 下载 FFmpeg 7.1.x full shared 包

LuZhi 当前依赖：

```toml
ffmpeg-next = { version = "7", optional = true }
```

因此优先使用 FFmpeg `7.1.x`，不要直接使用最新 `8.x`。主下载页可能只展示最新 release，例如 `ffmpeg-8.0.1-full_build-shared.7z`；这不适合作为当前 LuZhi 的 `FFMPEG_DIR`。

下载入口：

- 官方入口：<https://ffmpeg.org/download.html>
- 选择 Windows builds from gyan.dev。
- 如果 gyan.dev 主页面只展示最新 8.x，使用 GyanD GitHub Releases 历史版本。

推荐历史版本：

- <https://github.com/GyanD/codexffmpeg/releases/tag/7.1.1>
- 资产名称：`ffmpeg-7.1.1-full_build-shared.7z`
- 直接链接：<https://github.com/GyanD/codexffmpeg/releases/download/7.1.1/ffmpeg-7.1.1-full_build-shared.7z>

不要选择：

| 包 | 原因 |
|----|------|
| `essentials_build` | 可能不包含完整开发所需文件 |
| `ffmpeg-release-full-shared.7z` | 通常指向当前最新 release，可能是 8.x |
| `ffmpeg-8.x-full_build-shared.7z` | 与当前 `ffmpeg-next = "7"` 不匹配 |

#### 11.3.3 解压并整理目录

解压后目录应类似：

```text
C:\dev\toolchains\ffmpeg\7.1
  bin\
    ffmpeg.exe
    ffprobe.exe
    avcodec-*.dll
    avformat-*.dll
    avutil-*.dll
    swscale-*.dll
    swresample-*.dll
  include\
    libavcodec\
    libavformat\
    libavutil\
    libswscale\
    libswresample\
  lib\
    avcodec.lib
    avformat.lib
    avutil.lib
    swscale.lib
    swresample.lib
```

如果解压后得到的是类似 `ffmpeg-7.1.1-full_build-shared` 的目录，把它重命名或移动到：

```text
C:\dev\toolchains\ffmpeg\7.1
```

整理后检查：

```powershell
$ffmpegRoot = "C:\dev\toolchains\ffmpeg\7.1"

Test-Path "$ffmpegRoot\bin\ffmpeg.exe"
Test-Path "$ffmpegRoot\bin\ffprobe.exe"
Test-Path "$ffmpegRoot\include\libavformat\avformat.h"
Test-Path "$ffmpegRoot\lib\avformat.lib"
```

全部应返回 `True`。

#### 11.3.4 设置 FFmpeg 环境变量

设置：

```powershell
$ffmpegRoot = "C:\dev\toolchains\ffmpeg\7.1"
$ffmpegBin = "$ffmpegRoot\bin"

[Environment]::SetEnvironmentVariable("FFMPEG_DIR", $ffmpegRoot, "User")

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (($userPath -split ";") -notcontains $ffmpegBin) {
  [Environment]::SetEnvironmentVariable("Path", ($userPath.TrimEnd(";") + ";" + $ffmpegBin), "User")
}
```

关闭所有终端，重新打开 PowerShell 后复查：

```powershell
where ffmpeg
where ffprobe
ffmpeg -version
ffprobe -version
clang --version
echo $env:FFMPEG_DIR
echo $env:LIBCLANG_PATH
Test-Path "$env:FFMPEG_DIR\include\libavformat\avformat.h"
Test-Path "$env:FFMPEG_DIR\lib\avformat.lib"
Test-Path "$env:FFMPEG_DIR\bin"
```

全部返回正常后，FFmpeg 开发环境才算完整。

期望：

- `where ffmpeg` 的第一个路径是 `C:\dev\toolchains\ffmpeg\7.1\bin\ffmpeg.exe`。
- `ffmpeg -version` 显示 `7.1.x`。
- `FFMPEG_DIR` 是 `C:\dev\toolchains\ffmpeg\7.1`。
- header 与 `.lib` 检查均为 `True`。

如果其他项目支持显式配置 FFmpeg 路径，优先填写：

```text
C:\dev\toolchains\ffmpeg\7.1\bin\ffmpeg.exe
C:\dev\toolchains\ffmpeg\7.1\bin\ffprobe.exe
```

如果其他项目只读取 PATH，也会使用这套统一版本。

### 11.4 后续 Rust 链接验证

等 WindowsRecordingService 接入、非 macOS 编译保护移除或条件化后，再运行：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
```

如果失败，优先查看错误来自哪一层：

| 报错关键字 | 常见原因 |
|------------|----------|
| `bindgen` / `libclang` | LLVM 未安装或 `LIBCLANG_PATH` 未设置 |
| `avformat.h` not found | `FFMPEG_DIR\include` 不正确 |
| `avformat.lib` not found | `FFMPEG_DIR\lib` 不正确或下载包不含 dev libs |
| `LNK1112` / machine type conflict | FFmpeg 包架构与 Rust target 不一致 |
| 运行时找不到 DLL | `FFMPEG_DIR\bin` 未加入 PATH |

---

## 12. Windows 原生能力预检查

DXGI / WASAPI 开发前建议确认显示与音频设备状态。

检查 DirectX 诊断：

```powershell
dxdiag
```

确认：

- 显卡驱动正常。
- 显示设备正常。
- 声音播放设备正常。
- 麦克风输入设备存在。

麦克风权限：

```text
设置 -> 隐私和安全性 -> 麦克风
```

需要打开：

- 麦克风访问。
- 允许桌面应用访问麦克风。

WASAPI loopback 通常不需要额外隐私权限，但需要有可用的系统播放设备。

---

## 13. 最终环境验收清单

基础环境：

```powershell
git --version
node --version
npm --version
rustup show
rustc --version
cargo --version
cmd /c '"C:\Program Files\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat" && cl'
npx tauri --version
```

项目基础验证：

```powershell
npm install
npm test -- --run
npm run build
```

FFmpeg 环境验证：

```powershell
ffmpeg -version
ffprobe -version
clang --version
echo $env:FFMPEG_DIR
echo $env:LIBCLANG_PATH
Test-Path "$env:FFMPEG_DIR\include"
Test-Path "$env:FFMPEG_DIR\lib"
Test-Path "$env:FFMPEG_DIR\bin"
```

当前阶段预期可能失败：

```powershell
npm run tauri:dev
npm run tauri:dev:ffmpeg
cargo check --manifest-path src-tauri/Cargo.toml
```

如果失败信息指向 `WindowsRecordingService` 未接入或非 macOS 编译保护，说明环境已基本准备好，下一步应进入 Windows 平台代码实现。

---

## 14. 参考链接

- Tauri 2 prerequisites: <https://v2.tauri.app/start/prerequisites/>
- Microsoft WebView2 Runtime: <https://developer.microsoft.com/microsoft-edge/webview2/>
- FFmpeg official download: <https://ffmpeg.org/download.html>
- `ffmpeg-next` Windows build notes: <https://github.com/zmwangx/rust-ffmpeg/wiki/Notes-on-building>
