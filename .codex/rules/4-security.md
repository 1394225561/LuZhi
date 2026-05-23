# 安全规范

## 1. 安全权限、注入、秘钥处理

### 1.1 系统权限管理

- macOS：首次启动必须检测并引导 Screen Recording、Microphone 权限，不得在无权限下崩溃。
- Windows：需兼容 UAC 提权场景，录制受限窗口（如管理员权限的应用）时需给出明确提示。

### 1.2 秘钥与敏感信息

- **禁止**硬编码 Sentry DSN、PostHog Key、激活码校验私钥等。
- 敏感配置统一通过 Tauri 环境变量注入（`tauri.conf.json` -> `env`），或使用操作系统级 Keychain 存储授权信息。
- `.env` 文件必须加入 `.gitignore`。

### 1.3 安全防护

- Tauri 配置中，严禁开启 `allow-all` 权限，必须按最小权限原则配置 API Allowlist。
- 防注入：FFmpeg 命令行参数严禁直接拼接用户输入，必须使用结构化参数传递，防止命令注入。

---
