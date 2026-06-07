# bug 备忘清单

记录已知 bug。**重要**：每条 bug 修复后，都要总结对应的**预防规则**。

## 未解决

（暂无）

---

## 已解决

### BUG-0018: 历史录制侧栏折叠按钮和边框视觉异常 ✅ 已修复

**现象**：

1. 历史录制折叠/展开按钮外侧出现一圈不自然的浅色细线。
2. 侧栏展开后，侧栏左侧也出现相似的半圈浅色细线。
3. 侧栏折叠时，窗口右边框残留一条接近窗口高度的细线。
4. 侧栏折叠时，折叠/展开按钮贴在窗口最右侧，距离开始录制面板过远，用户不易注意到。

**期望**：折叠/展开按钮无多余边框线；折叠侧栏不残留右边框细线；折叠按钮靠近空闲页录制面板右侧，仍保持可发现性。

**根因**：

1. `RecordingSidebar` 的 toggle 按钮使用 `border border-border/60` 和 `shadow-lg`，展开时按钮贴在侧栏左边缘，圆角边框视觉上形成半圈浅色线。
2. 侧栏容器在折叠状态仍保留 `border-l border-border/60`，即使宽度为 `w-0`，左边框仍会渲染成整高细线。
3. 折叠状态使用 `right-0`，按钮被固定在窗口右边缘，而空闲页录制面板居中显示，两者距离过远。

**修复**：

1. 移除 toggle 按钮常态边框和阴影，仅保留无边框 surface 背景、hover 高亮和键盘 focus ring。
2. 侧栏容器改为仅展开状态保留阴影；折叠状态显式设置 `border-l-0`、`shadow-none`、`pointer-events-none`。
3. 折叠状态 toggle 位置改为 `right-[max(12px,calc(50%_-_208px))]`，使按钮靠近居中的录制面板右侧，同时在窄窗口保留最小右边距。
4. 新增前端回归测试覆盖折叠状态无残留边框、toggle 无常态边框/阴影、折叠位置不再贴右边框。

**预防规则**：

26. 折叠为 `w-0` 的侧栏不能在基础 class 中保留可见边框、阴影或可交互区域；折叠状态必须显式关闭这些视觉与交互残留。
27. 悬浮 toggle 按钮若与面板边缘相贴，常态不应使用完整边框，否则圆角边框会与面板边缘叠加成异常半圈线。
28. 空闲页辅助入口按钮不能默认贴窗口最远边缘；位置应参照主操作面板，保证可发现性。

### BUG-0017: 历史录制记录重新导出失败 ✅ 已解决-人工验证通过

**现象**：

1. 启动应用后，不进行录制，直接点击历史记录，跳转预览美化界面，此时点击导出会报错：`导出失败，请重试或检查录制素材。`
2. 启动应用后，进行一次录制（记为 A 视频），此时，无论后续触发哪条历史记录的导出，实际导出的都是 A 视频。

**期望**：正确美化导出历史记录对应的视频。

**根因**：导出链路中有三个函数从 `service` 状态读取路径，历史记录导出时 service 没有活跃会话数据，全部失败：

1. **`build_cursor_effect_timeline`** 读 `service.last_cursor_metadata_path()` → 报错"没有可用的光标元数据"
2. **`build_cut_timeline`** 读 `service.last_trim_metadata_path()` → 报错"没有可用的裁剪元数据"
3. **`export_video`** 读 `service.last_recording_output_path()` → 报错"没有可用的原始录制文件"或导出错误的视频

第一轮修复只修了第 3 点（source_path），遗漏了前两个 builder 函数，导致修复不完整。

**修复**：

1. `build_cursor_effect_timeline`、`build_cut_timeline`、`export_video` 三个函数统一增加 `recording_id: Option<String>` 参数
2. 当 `recording_id` 提供时（历史记录导出），从 `RecordingLibrary` 查询对应的元数据路径，跳过 session 验证和 service 回写
3. 当 `recording_id` 为 `None` 时（新鲜录制导出），保持原有的 `service` 状态读取逻辑
4. 前端 `buildCursorEffectTimeline()`、`buildCutTimeline()`、`exportVideo()` 统一增加可选 `recordingId` 参数
5. `PreviewView` 接收 `recordingId` prop 并传递给所有 builder 和 export 调用
6. `App.tsx` 在 `handleSelectRecording` 中保存 `selectedRecordingId`，在新鲜录制停止和返回 idle 时清除

**预防规则**：

23. 导出等关键操作的源数据路径必须由调用方显式传入，不能隐式依赖 service 内部状态。当 UI 允许从不同入口（新鲜录制 vs 历史记录）触发同一操作时，必须确保后端能区分数据来源。
24. 前后端状态一致性：前端通过 `getRecordingContext` 获取的路径仅用于 UI 展示，如果后端操作（如导出）也需要这些路径，必须通过参数显式传递或同步更新后端状态，不能假设两端状态自动一致。
25. 调用链路完整性审查：修复"路径来源不一致"类 bug 时，必须沿调用链向上追溯所有读取 service 状态的函数，不能只修最末端的 consumer。遗漏中间节点会导致部分修复（现象变为更隐蔽的报错）。

### BUG-0015: 历史录制列表为空 ✅ 已解决-人工验证通过

**现象**：录制结束，从预览美化界面点击返回，历史录制列表为空，并没有加载出历史数据。

**期望**：返回或者启动应用，能够正常加载出历史数据。

**根因**：双层问题叠加导致录制结果无法入库。

1. **`stop_recording` 中存在 `if !resp.failed` 守卫**：`RecordingFinalizeGuard` 收集的任何非关键错误（光标元数据写入失败、裁剪元数据写入失败、麦克风停止失败等）都会设置 `failed = true`，导致注册逻辑被完全跳过。视频文件正常生成且可播放，但从未被加入录制库索引。
2. **`register()` 要求 `cursor_metadata_path` 和 `effect_timeline_path` 必须为 `Some` 且文件存在**：但 `effect_timeline_path` 仅在美化流程中设置，录制停止时始终为 `None`；`cursor_metadata_path` 在光标追踪未启动或写入失败时也为 `None`。这导致即使绕过了第一层守卫，注册仍然失败。

**修复**（提交 `2b094f2`）：

1. 移除 `stop_recording` 中的 `if !resp.failed` 守卫，只要 `output_path` 存在就执行注册
2. 将 `LibraryEntry` 中的 `cursor_metadata_path` 和 `effect_timeline_path` 改为 `Option<PathBuf>`，与 `trim_metadata_path`/`cut_timeline_path` 保持一致
3. 更新 `register()` —— 所有伴生文件可选，仅视频文件为必选，不存在的文件存为 `None`
4. 更新 `get_recording()` —— 缺失的伴生文件返回空 JSON
5. 更新 `delete()` —— 仅删除 `Some` 的伴生文件
6. 更新 `repair_on_startup()` —— 仅检查 `video_path` 是否存在
7. 更新 `get_recording_context()` —— 可选路径用 `unwrap_or_default()` 处理

**预防规则**：

1. **禁止用 `failed` 守卫跳过关键业务逻辑**：录制入库是关键路径，不应被非关键错误（伴生文件写入失败）阻断。关键路径注册应与错误上报解耦。
2. **伴生文件一律设计为可选**：录制产物的伴生文件（cursor_metadata、effect_timeline、trim_metadata、cut_timeline）均应为 `Option`，仅视频文件为必选。注册、查询、删除、修复全链路必须统一处理 `None` 情况。
3. **RAII 守卫的错误不应阻断业务流**：`RecordingFinalizeGuard` 收集的错误用于上报和诊断，不应阻止录制结果入库。业务层需区分"录制失败"与"录制成功但有非关键错误"。

---

### BUG-0016: 导入失败 ✅ 已修复

**现象**：导入落盘的原始录制视频，报错：非本应用录制的视频。实际上，确实是本应用录制的视频。

- 视频路径：`/Users/root-mac/Downloads/luzhirecord/recording-1780726583259-0.mp4`
- 其他相关文件：
  - `/Users/root-mac/Downloads/luzhirecord/recording-1780726583259-0-bilibili-export-1.mp4`
  - `/Users/root-mac/Downloads/luzhirecord/cursor-effects-1780726618826-0.json`
  - `/Users/root-mac/Downloads/luzhirecord/cursor-metadata-1780726614720-0.json`
  - `/Users/root-mac/Downloads/luzhirecord/trim-metadata-1780726614724-0.json`
- 只有这 5 个文件，并没有落盘更多文件。

**期望**：能够导入成功。

**根因**：`import()` 函数使用视频文件名的时间戳构造配套文件名（如 `cursor-metadata-1780726583259-0.json`），但实际配套文件使用录制开始时的时间戳（如 `cursor-metadata-1780726614720-0.json`），两者不同导致文件查找失败。

**修复**：

- 新增 `find_companion_file()` 辅助函数，按前缀和序列号在目录中搜索配套文件
- 配套文件不再要求与视频文件时间戳完全匹配，而是通过 `cursor-metadata-*-{seq}.json` 模式匹配
- `trim_metadata_path` 和 `cut_timeline_path` 改为可选（用户未启用自动裁剪时不存在）

**预防规则**：

1. 配套文件发现不得依赖视频文件名时间戳，应按前缀+序列号模式搜索
2. 导入校验应区分必需文件和可选文件

---

### BUG-0014: 美化界面拖拽触发范围太广，需要优化 ✅ 已解决

**现象**：所有区域都可以触发窗口拖拽，导致用户无法通过光标选中文字进行复制。

**期望**：美化界面只有空白区域可以触发窗口拖拽，按钮、文本区域，不会触发窗口拖拽。

**根因**：

1. `App.tsx` 的程序化窗口拖拽监听使用 `target.closest('[data-tauri-drag-region]')` 判断拖拽区域。预览美化界面根容器带有 `data-tauri-drag-region="deep"`，因此标题、说明文案等普通文本节点都会继承拖拽能力。
2. 同一文件中全局监听 `selectstart` 并 `preventDefault()`，即使拖拽范围修正，用户仍无法选择文本进行复制。
3. 第一轮只收窄了程序化 `startDragging()` 入口，但仍在根容器保留 `data-tauri-drag-region="deep"`。真实 Tauri WebView 会原生识别该属性，导出成功后显示的保存路径仍位于该祖先节点下，因此拖选路径文字时继续触发窗口拖拽。
4. 第二轮把自定义拖拽标记放在预览页根容器，并要求事件目标本身带有该标记；但主预览区域和右侧边栏覆盖了根容器，用户实际点击空白时命中的是内容容器，导致美化界面所有区域都无法拖拽。

**修复**：

1. 移除 `src` 中所有真实 `data-tauri-drag-region` 用法，避免 Tauri 原生拖拽机制绕过前端过滤。
2. 保留右键菜单禁用，移除全局 `selectstart` 阻止，允许预览美化界面文本选择复制。
3. 将 `data-luzhi-drag-region` 放到美化界面的两个内容容器：主预览区域和右侧边栏。
4. 程序化拖拽允许命中自定义拖拽容器内的空白区域；但当事件目标是按钮、输入控件、视频控件、可交互角色或文本元素时不触发拖拽。
5. 新增前端回归测试覆盖：主预览区域空白可拖、右侧边栏空白可拖、文本不触发拖拽、预览文本可触发选择、导出保存路径不位于 Tauri 原生 drag-region 祖先下且不会触发 `startDragging()`。

**预防规则**：

1. 无边框窗口拖拽不得在大容器上使用 Tauri 原生 `data-tauri-drag-region="deep"`；需要过滤子级文本/控件时，应使用应用自定义标记并由显式事件逻辑触发 `startDragging()`。
2. 为修复拖拽范围新增测试时，必须同时覆盖负向用例（文本/控件不拖拽）和正向用例（主预览区域、右侧边栏空白仍可拖拽）。
3. 需要支持复制的信息区域不得被全局 `selectstart` 拦截；若未来需要禁选，只能限定在明确不可复制的拖拽面上。
4. 导出路径、错误信息、诊断信息等用户可能复制的文本，必须测试其祖先链不包含 Tauri 原生 drag-region。
5. 自定义拖拽标记必须放在用户实际点击的内容容器上；如果只放在被子容器完全覆盖的根节点上，空白拖拽会失效。

---

### BUG-0013: 结束录制报错 ✅ 已解决

**现象**：录制前只开启系统音频，没有开启麦克风采集，但是录制过程中系统并没有音频输出，点击结束录制会报错。如果有系统音频输出，比如开启音乐播放，结束录制、导出功能都正常。

**期望**：开启了系统音频采集，哪怕录制全程都是静音状态，也能正常录制、导出。

**根因**：`RequestedAudioContract` 验证逻辑假设"如果用户请求了音频录制，那么录制的音频不应该是静音的"。当用户仅开启系统音频采集，但录制过程中系统确实没有音频输出时，录制的音频确实是静音的（RMS=0.000000, peak=0.000000），导致 `validate_source_artifact_with_audio_contract` 验证失败。

**修复**：

1. **`RequestedAudioContract` 结构体**：新增 `allow_silent_if_system_only` 字段（默认 `false`），用于标识是否允许仅系统音频场景下的静音音频。
2. **`should_allow_silent_audio()` 方法**：新增方法，当 `allow_silent_if_system_only=true` 且仅请求了系统音频（无麦克风）时返回 `true`。
3. **验证逻辑**：在 `validate_source_artifact_with_audio_contract` 和 `validate_export_artifact_with_audio_contract` 中，当 `should_allow_silent_audio()` 返回 `true` 时，允许静音音频通过验证（打印日志但不报错）。
4. **调用方设置**：在 `macos_service.rs` 和 `lib.rs` 中，当 `requested_system_audio=true` 且 `requested_microphone=false` 时，设置 `allow_silent_if_system_only=true`。

**预防规则**：

1. 音频 contract 验证必须区分"用户请求了音频但完全没有音频轨道"（错误）和"用户请求了音频且有音频轨道但内容静音"（可能正常）两种场景。
2. 当仅请求系统音频时，必须允许静音音频通过验证，因为系统可能确实没有音频输出。
3. 验证逻辑的假设必须与实际使用场景对齐，不能假设"请求了音频就一定有音频输出"。
4. 新增 contract 字段时，必须同步更新所有调用方和导出验证逻辑。

---

### BUG-0010: 导出的视频光标定位不对 ✅ 已解决-人工验证通过

**现象**：经过美化后导出的视频，光标的定位显示向左上方产生偏移了。

**期望**：美化的光标与源视频的光标是精确定位，没有偏移。

**根因**：`MacCursorSource.snapshot()` 返回 macOS 全局屏幕坐标（`CGEventGetLocation`），但 `CursorOverlayRenderer` 假设坐标已在源视频像素空间。缺少 display origin/contentRect/pointPixelScale/stream size 的坐标归一化。

**修复**：新增 `CaptureGeometry`、`CursorCoordinateMapper`，从 SCDisplay 读取显示几何信息，在 `CursorMetadataRecorder` 中归一化坐标。

**预防规则**：

1. cursor metadata 必须保存或使用与 source video 一致的坐标空间，不能把全局屏幕坐标直接交给 exporter。
2. ScreenCaptureKit display origin、contentRect、pointPixelScale、stream output size 必须进入 cursor 坐标归一化。
3. cursor glyph 绘制必须以 hotspot 对齐目标点，不能以图标左上角或视觉中心对齐。
4. 多显示器、Retina scale、CenterCrop/FitWithBars 必须有坐标回归测试或人工门禁。

#### BUG-0010_1: 第 1 轮人工验证结果

**现象**：经过美化后导出的视频，光标的定位依旧产生了很大的偏移，但是这次的偏移方向是在垂直向上的方向，偏移幅度很大，超过了半个屏幕的高度。

**期望**：美化的光标与源视频的光标是精确定位，没有偏移。

**根因**：`CursorCoordinateMapper` 中 `flip_y: true` 硬编码无条件翻转 Y 轴。`CGEventGetLocation()` 和 `SCDisplay.frame()` 使用同一坐标系（top-left origin, Y 向下），不需要翻转。

**修复**：移除 `flip_y` 字段和翻转逻辑，简化 `map()` 为直接线性映射。新增 7 个 mapper 测试覆盖 top-left、bottom-right、下半屏、负 origin、Retina 四角场景。

#### BUG-0010_2: 第 2 轮整改状态

✅ 已修复-第二轮待人工验证

**本轮整改内容**：

1. 移除无条件 Y 轴翻转 — `CGEventGetLocation` 与 SCK 使用同一 top-down 坐标系
2. 新增 mapper 四角和负 origin 测试锁定 BUG-0010 Y 轴错误
3. 添加 SCDisplay 几何诊断日志用于人工确认坐标系
4. CursorClick 坐标归一化 — 与 CursorSample 使用同一 source video 坐标空间

**新增预防规则**：

5. mapper 测试不能只测中心点，必须测 top/bottom。
6. Y 轴翻转必须有真实设备证据或显式 geometry 语义，不能硬编码假设。
7. `CursorSample` 和 `CursorClick` 必须处于同一 source video pixel 坐标空间。

#### BUG-0010_3: 第 2 轮人工验证结果

**现象**：经过美化后导出的视频，光标的定位依旧存在偏移。目前光标在垂直方向上的高度定位是正确的，但是水平方向上的定位偶尔准确，偶尔偏左，偶尔偏右。

**期望**：美化的光标与源视频的光标是精确定位，没有偏移。

#### BUG-0010_4: 第 3 轮整改状态

✅ 已修复-第三轮待人工验证

**本轮根因**：第二轮新增同步 AX kind 查询后，`MacCursorSource.snapshot()` 先读取坐标，再执行可能阻塞的 AX hit-test；`CursorMetadataRuntime` 在 `snapshot()` 返回后才写入 timestamp。坐标采样时刻和记录 timestamp 不再一致，横向移动时会出现方向相关的左右偏移。

**本轮整改内容**：

1. timestamp 移到 `snapshot()` 调用前 — 避免 AX 查询耗时污染坐标时间戳
2. AX 查询限频 10Hz — `MacCursorSource` 持有 `SessionClock`，kind 缓存 100ms TTL
3. `CursorSnapshot` 携带 `captured_at_nanos` — 精确记录坐标采样时刻
4. 首帧 CVPixelBuffer 实际尺寸诊断日志

**新增预防规则**：

8. `snapshot()` 内任何可能阻塞的操作（如 AX 查询）不得污染坐标采样 timestamp。
9. cursor position 和 cursor kind 对时间精度要求不同；position 必须贴近视频帧时间，kind 可以低频缓存。
10. 首帧视频到达时必须记录 actual CVPixelBuffer 尺寸并与 CaptureGeometry 对比。

#### BUG-0010_5: 第 3 轮人工验证结果

**现象**：经过美化后导出的视频，光标的定位精读有改善，但是依旧存在偏移。目前光标在垂直方向上的高度定位是正确的，但是水平方向上的定位在录屏刚开始时是准确的，经过几段移动后，水平方向上的定位就开始出现向右偏移了。

**期望**：美化的光标与源视频的光标是精确定位，没有偏移。

#### BUG-0010_6: 第 4 轮整改状态

✅ 已修复-第四轮待人工验证

**本轮根因**：视频帧 PTS、cursor sample timestamp、effect timeline timestamp、导出 overlay source timestamp 缺少显式对齐契约；smoothing/Bezier 插值可能污染定位验收；source MP4 第一帧 PTS 不为 0 时 overlay 时间轴错位。

**本轮整改内容**：

1. 新增 MediaTimelineDiagnostics — 记录视频首帧 PTS origin、首帧 normalized timestamp、首末 cursor timestamp、帧数/样本数对比
2. 新增 CursorTimingDiagnostics — 记录采样间隔 min/max/avg、几何外样本数、Accessibility 权限状态
3. trim_exporter overlay timestamp 显式减去 source MP4 第一帧 PTS origin — 对齐 cursor timeline
4. 新增 raw positioning 验收模式 — 关闭 smoothing/Bezier/magnification，仅渲染 glyph
5. CursorTimingDiagnostics 写入 RecordingMetadata — stop 时完整结构化摘要
6. 首帧 actual CVPixelBuffer size 写入 metadata（非仅 stderr）

**新增预防规则**：

11. video frame PTS、cursor sample timestamp、effect timeline timestamp、export overlay source timestamp 必须共享同一 source media timebase；任何 offset 都必须写入 metadata 并由测试验证。
12. source artifact 第一帧 PTS 和 metadata 首帧/末帧时间必须有诊断对比；导出 overlay 不得隐式假设 raw_pts 从 0 开始。
13. 光标定位验收必须先在 raw positioning mode 下进行，禁止 smoothing/magnification 影响坐标正确性判断。
14. cursor diagnostics 必须落盘到 sidecar，不能只打印到 stderr。

#### BUG-0010_7: 第 4 轮人工验证结果

**现象**：经过美化后导出的视频，光标的定位精读有改善，但是依旧存在偏移。目前光标在垂直方向上的定位是正确的，但是水平方向的定位偏移有如下规律：

- 当我大浮动向左移动时，静止后的光标水平方向上向左产生了偏移
- 当我大浮动向右移动时，静止后的光标水平方向上向右产生了偏移

**期望**：美化的光标与源视频的光标是精确定位，没有偏移。

#### BUG-0010_8: 第 5 轮整改状态

✅ 第五轮人工验证通过，已解决

**本轮根因**：水平漂移仅在开启平滑/贝塞尔时出现，说明问题不在坐标映射或 PTS 对齐，而在光标平滑算法本身。(1) 简单移动平均在光标移动时产生恒定滞后（~2-4 帧），停止后需 `window_size/2` 帧才能收敛；(2) Catmull-Rom Bezier 控制点在方向变化时产生过冲，放大了移动平均的滞后，导致"沿移动方向漂移"的现象。

**本轮整改内容**：

1. 移动平均替换为 EMA（指数移动平均）— alpha=0.4，停止后 ~2-3 帧收敛（原移动平均需 ~4-5 帧）
2. Catmull-Rom Bezier 替换为单调三次 Hermite 插值（Fritsch-Carlson 方法）— 数学保证不过冲，消除方向变化时的累积漂移
3. 新增 3 个漂移回归测试：快速移动停止后无过冲、方向反转后无累积漂移、阶跃函数单调性验证
4. 清理未使用的 `distance_between` 函数

**新增预防规则**：

20. 光标平滑算法必须保证单调性：在方向变化或停止时，插值结果不得超出相邻样本值的范围。Catmull-Rom 等非单调插值方法禁止用于光标位置平滑。
21. 平滑算法的收敛速度必须在 3 帧以内：光标停止后，平滑位置必须在 3 帧内收敛到实际位置，不能有可见的持续漂移。
22. 光标平滑回归测试必须覆盖：快速移动停止、方向反转、阶跃函数三种场景，验证无过冲和无累积漂移。

---

### BUG-0011: 美化后的光标不好看 ✅ 已解决-第 6 轮人工验证通过

**现象**：经过美化后导出的视频，显示的光标是个白色的圆，不好看。

**期望**：针对光标 target 绘制不同的形态。

1. 普通状态为 macOS 上的黑底白边箭头
2. 可点击区域时，形状为白底黑边的手的形状
3. 输入框区域，形状为黑底白边的 "I" 的形状

**根因**：`CursorSample`/`CursorFrame` 没有 `kind` 字段，`CursorOverlayRenderer` 固定画白色圆点。

**修复**：新增 `CursorKind` 枚举（Arrow/Hand/IBeam），扩展 `CursorSample`/`CursorFrame`，保留 kind 通过引擎管线，用静态 bitmask glyph 渲染替换圆点。

**预防规则**：

1. cursor timeline 必须携带稳定的 cursor kind 或 glyph 信息；renderer 不得固定画圆点冒充系统 cursor。
2. target-aware cursor 识别失败时必须 fallback 为 Arrow，不能阻塞录制或导出。
3. cursor glyph 必须有 hotspot metadata，并由测试验证 hotspot 对齐。
4. cursor asset/bitmask 更新必须配套像素级或 snapshot-like 回归测试。

#### BUG-0011_1: 第 1 轮人工验证结果

**现象**：经过美化后导出的视频，显示的光标在任何情况下都是一个白底黑边的箭头。

**期望**：

1. 普通桌面 — 导出光标为黑底白边箭头
2. 悬停按钮/链接 — 导出光标为白底黑边手形（手形的 glyph 绘制需要更精致一些：大拇指和食指伸直，其他三个手指向掌心弯曲，视觉效果很短）
3. 悬停文本框 — 导出光标为黑底白边 I-beam

**根因**：

1. `CursorSnapshot` 没有 `kind` 字段，`record_snapshot()` 固定写 `CursorKind::Arrow`。
2. Arrow/IBeam glyph 颜色与验收相反（白底黑边 → 应为黑底白边）。

**修复**：

1. 扩展 `CursorSnapshot` 携带 `kind`，`record_snapshot()` 使用 `snapshot.kind`。
2. 实现 macOS CursorKind provider — `AXUIElementCopyElementAtPosition` Accessibility hit-test 识别 Arrow/Hand/IBeam。
3. Arrow glyph 改为白底黑边，IBeam glyph 改为白底黑边。
4. 新增像素级颜色契约测试。

#### BUG-0011_2: 第 2 轮整改状态

✅ 已修复-第二轮待人工验证

**本轮整改内容**：

1. CursorSnapshot 携带 kind 字段，record_snapshot 使用真实 kind
2. 实现 macOS CursorKind provider — Accessibility hit-test 识别 Arrow/Hand/IBeam
3. 修正 Arrow/IBeam glyph 颜色 — 黑底白边符合验收要求
4. 新增像素级颜色契约测试 — Arrow/Hand/IBeam 颜色和渲染验证

**新增预防规则**：

8. `CursorKind` enum 存在不代表 target-aware 已完成；必须有 kind source。
9. renderer 测试必须验证具体 glyph 类型和颜色，不得只检查"像素非零"。
10. target-aware 查询失败必须 fallback Arrow，但 fallback 不能掩盖全部样本都未识别的问题，必须有诊断计数。

#### BUG-0011_3: 第 2 轮人工验证结果

**现象**：经过美化后导出的视频，显示的光标在任何情况下都是一个黑底白边箭头。

**期望**：

1. 普通桌面 — 导出光标为黑底白边箭头（箭头的尾部加上把柄形状）
2. 悬停按钮/链接 — 导出光标为白底黑边手形（手形的 glyph 绘制需要更精致一些：大拇指和食指伸直，其他三个手指向掌心弯曲，视觉效果很短）
3. 悬停文本框 — 导出光标为黑底白边 I-beam

#### BUG-0011_4: 第 3 轮整改状态

✅ 已修复-第三轮待人工验证

**本轮根因**：AX provider 使用 `AXUIElementCreateApplication(0)` 作为 system-wide root，与 SDK 语义冲突。`AXUIElementCopyElementAtPosition()` 查询失败或被错误作用域限制，所有样本 fallback Arrow。

**本轮整改内容**：

1. AX provider 改用 `AXUIElementCreateSystemWide()` — 修复跨应用 hit-test
2. 新增 `AXUIElementSetMessagingTimeout(50ms)` — 避免阻塞 cursor runtime
3. CursorKind 分类扩展 — parent chain 回溯 3 层 + AXPress action 检查
4. `AXStaticText` 不再默认判为 IBeam — 只有 `AXTextField`/`AXTextArea` 才判 IBeam
5. CursorKindDiagnostics 写入 RecordingMetadata — stop 时打印结构化日志
6. Hand/IBeam rendered Y plane 测试

**新增预防规则**：

11. AX hit-test 必须使用 `AXUIElementCreateSystemWide()`，不能用 `AXUIElementCreateApplication(0)`。
12. AX 查询必须设置 messaging timeout（建议 50ms），避免阻塞 cursor runtime。
13. CursorKind 分类必须支持 parent chain 回溯（至少 3 层），覆盖浏览器/Electron/Tauri 场景。
14. `AXStaticText` 不等于可编辑文本；只有 `AXTextField`/`AXTextArea` 或明确 editable 才判 IBeam。

#### BUG-0011_5: 第 3 轮人工验证结果

**现象**：完全没有改善，经过美化后导出的视频，显示的光标在任何情况下都是一个黑底白边箭头。

**期望**：

1. 普通桌面 — 导出光标为黑底白边箭头（**箭头的尾部加上把柄形状**）
2. 悬停按钮/链接 — 导出光标为白底黑边手形（手形的 glyph 绘制需要更精致一些：大拇指和食指伸直，其他三个手指向掌心弯曲，视觉效果很短）
3. 悬停文本框 — 导出光标为黑底白边 I-beam

#### BUG-0011_6: 第 4 轮整改状态

✅ 已修复-第四轮待人工验证

**本轮根因**：(1) AX 查询依赖 Accessibility 权限，但权限模型未检查该权限；(2) AX 全局失败/回退计数未合并到 metadata，diagnostics 显示 0 掩盖了全部 fallback 事实；(3) CursorKindProvider trait 未接入生产路径，测试与生产脱节；(4) Arrow glyph 缺少尾部把柄。

**本轮整改内容**：

1. 新增 Accessibility 权限检查 — `AXIsProcessTrusted()`，前端未授权时显示提示
2. 合并 AX 全局计数器到 metadata — `cursor_kind_diagnostics_merged()` 确保 failure/fallback 真实落盘
3. CursorKindProvider 注入 MacCursorSource — 删除重复 TTL，统一 provider 路径
4. AX 分类限频采样日志 — role chain、result code、query duration，每秒最多 2 条
5. Arrow glyph 添加尾部把柄 — 匹配 macOS 原生箭头光标外形
6. 删除废弃的 AXUIElementCreateApplication FFI 声明

**新增预防规则**：

15. target-aware cursor 依赖 Accessibility 时，Accessibility 权限必须进入录制前门禁和 metadata；未授权不能静默表现为全 Arrow。
16. AX failure/fallback/kind distribution 必须来自实际 provider 并写入 RecordingMetadata，不能只统计 recorder 最终 kind。
17. CursorKindProvider 必须可注入并覆盖生产路径，避免 mock 测试与真实采样脱节。
18. 每次 target-aware 整改必须提供 role/action/classification 采样日志或等价诊断，证明 Hand/IBeam 的来源。
19. glyph 形状变更必须配套 hotspot、颜色、关键形状区域的像素级测试。

#### BUG-0011_7: 第 4 轮人工验证结果

**现象**：完全没有改善，经过美化后导出的视频，显示的光标在任何情况下都是一个黑底白边箭头，并且箭头光标没有尾柄。

**期望**：

1. 普通桌面 — 导出光标为黑底白边箭头（**箭头的尾部加上把柄形状**）
2. 悬停按钮/链接 — 导出光标为白底黑边手形（手形的 glyph 绘制需要更精致一些：大拇指和食指伸直，其他三个手指向掌心弯曲，视觉效果很短）
3. 悬停文本框 — 导出光标为黑底白边 I-beam

#### BUG-0011_8: 第 5 轮整改状态

⚠️ 第五轮方案仍不可靠，已由第 6 轮替换生产主路径

**本轮根因**：`AXUIElementCopyElementAtPosition` 的坐标空间与 `CGEventGetLocation` 不同。CGEvent 使用 top-left origin（Y 向下递增），AX API 使用 bottom-left origin（Y 向上递增）。未翻转 Y 坐标导致 AX 查询始终命中屏幕顶部的 AXMenuBar，从未到达下方的按钮/文本框/链接。

**证据**：日志显示所有查询均返回 `role_chain=["AXMenuBar", "AXApplication"]`，diagnostics `arrow=436 hand=0 ibeam=0`，无论光标在屏幕什么位置。

**本轮整改内容**：

1. `MacCursorKindProvider` 新增 `display_height` 字段 — 通过 `CGDisplayBounds(CGMainDisplayID())` 获取主显示器高度
2. `query()` 方法在调用 AX API 前翻转 Y 坐标 — `ax_y = display_height - global_y`
3. `query_cursor_kind()` 签名更新为 `ax_y: f64` — 明确表示已翻转的 AX 坐标空间

**新增预防规则**：

23. `AXUIElementCopyElementAtPosition` 坐标空间与 `CGEventGetLocation` 不同（Y 轴方向相反）。调用 AX API 前必须用显示器高度翻转 Y 坐标。
24. cursor kind 分类诊断日志必须同时显示 CGEvent 坐标和 AX 坐标，便于排查坐标空间问题。

#### BUG-0011_9: 第 6 轮整改状态

✅ 第 6 轮人工验证通过；FFI code review findings 已收口

**本轮人工反馈**：第 5 轮后，美化导出视频依旧无法正确切换光标形态。特殊场景：四指上划调出 macOS Mission Control 时，macOS 实际显示 Arrow，但导出的美化视频会把光标渲染成 Hand。

**本轮根因**：AX role inference 不是系统光标形态的真实来源。`AXUIElementCopyElementAtPosition` 只能说明某个坐标下的辅助功能元素可能可交互，不能证明 macOS 当前显示的是 Hand/IBeam。WebView 内容对 AX hit-test 不透明会导致漏判；Mission Control 等系统 UI 又可能命中可交互 AX 元素而误判 Hand。因此继续修 AX 坐标或 role chain 只能缓解局部症状，无法保证导出光标和系统真实光标一致。

**本轮整改内容**：

1. `MacCursorKindProvider` 改为优先读取 `NSCursor.currentSystemCursor`，以 macOS 当前实际显示的系统光标作为生产主来源。
2. 新增 `SystemCursorShape` 映射：`arrowCursor -> Arrow`、`pointingHandCursor -> Hand`、`IBeamCursor -> IBeam`。
3. 系统光标匹配先尝试对象指针/`isEqual:`，再比较 hot spot + TIFF 图像数据，兼容 `currentSystemCursor` 返回等价 cursor 对象的情况。
4. 未知系统光标或读取失败时 fallback 为 Arrow，避免未知状态被 AX role 误判为 Hand。
5. NSCursor 读取包裹 `NSAutoreleasePool`，避免后台 10Hz 轮询积累 autorelease 对象。
6. 移除前端“需要辅助功能权限才能识别手形/文本光标”的误导提示；NSCursor-based cursor kind 不依赖 Accessibility 权限。
7. 新增回归测试：系统 Arrow 不会被推断为 Hand，覆盖 Mission Control false positive；未知系统光标不能误判为 Hand。

**新增预防规则**：

25. cursor kind 的生产主来源必须是系统实际显示的 cursor shape，不能把 AX role/action 当作真实 cursor shape。
26. AX hit-test 可作为诊断信息，但不得在生产路径中覆盖 `NSCursor.currentSystemCursor` 的 Arrow/Hand/IBeam 结果。
27. 未知系统 cursor shape 必须 fallback Arrow，不能为了“更智能”猜测为 Hand/IBeam。
28. AppKit cursor 轮询必须有 autorelease pool 或等价释放边界，避免后台线程长期轮询造成 autorelease 对象堆积。
29. Mission Control/桌面调度场景必须进入人工门禁：macOS 显示 Arrow 时，导出美化视频也必须为 Arrow。

#### BUG-0011_10: 第 6 轮 FFI Code Review 收口

✅ 已修复并通过自动化验证

**本轮背景**：人工验证确认第 6 轮 NSCursor 方案效果正常后，对本次改动涉及的 FFI 安全性和正确性做了专项 code review。Review 重点为 Objective-C selector 可用性、异常安全、autorelease 生命周期、C string 类型边界、AppKit 线程边界和 clippy 近邻告警。

**本轮整改内容**：

1. 新增 `CursorMainThreadDispatcher`，生产录制链路通过 Tauri `AppHandle::run_on_main_thread` 执行 AppKit cursor 读取。
2. `MacCursorKindProvider` 构造时必须接收 main-thread dispatcher，避免生产路径在 cursor metadata 后台线程直接调用 AppKit。
3. `NSCursor.currentSystemCursor`、class cursor、`image`、`hotSpot`、`TIFFRepresentation`、`isEqual:`、`isEqualToData:` 均在 `objc_msgSend` 前检查 class/instance method 是否存在；缺失时 fail closed 并 fallback Arrow。
4. ObjC selector 参数改为 `&CStr` / `c"..."`，不再把 `&[u8]` 当作 C string 传给 `sel_registerName`。
5. autorelease pool 改为 runtime `objc_autoreleasePoolPush/Pop`，每次系统 cursor 读取都有释放边界。
6. legacy AX 的 `NSWorkspace` ObjC 调用同步改用 guarded helper，避免留下另一条未防护消息发送路径。
7. 新增测试覆盖 main-thread reader 调度成功/失败行为。

**新增预防规则**：

30. ObjC `objc_msgSend` 前必须验证 selector 对应 class/instance method 存在；缺失时必须 fallback，不得让 Objective-C exception 跨 Rust FFI 边界。
31. AppKit cursor shape 读取必须通过主线程 dispatcher 或等价主线程机制执行；后台 cursor metadata 线程不得直接调用 AppKit。
32. ObjC selector/C string FFI 参数必须使用 `CStr` 或 `c"..."` 字面量，不得用裸 `&[u8]` 作为 C string。
33. 高频 AppKit/Objective-C 轮询必须有强制 autorelease pool 边界；不得在 pool 创建失败或缺失时继续执行会产生 autoreleased 对象的调用。

---

### BUG-0012: 点击导出按钮后的交互不够优雅 ✅ 已解决-人工验证通过

**现象**：点击导出，没有进度条反馈，直接从 0% 变成导出完成。

**期望**：能够在界面上显示导出的百分比进度。

**根因**：`FfmpegTrimExporter` 按 keep segment 上报进度（`(seg_idx+1)*99/total_keeps`），自动裁剪关闭时只有一个 keep segment，表现为 0% 停很久然后直接完成。

**修复**：

1. 升级为 frame/time 级进度：每帧视频编码后按 `processed_nanos / total_keep_nanos` 上报
2. 准备阶段进度：cursor timeline 0→5%，cut timeline 5→10%
3. 范围映射：FFmpeg 导出 1-99 映射到整体 10-99%
4. 前端 terminal error 显示：失败时展示错误详情

**预防规则**：

1. export progress 不能只按 keep segment 上报；单段完整导出也必须产生中间进度。
2. export progress 必须单调递增，且 100% 只能在导出和 artifact validation 成功后发送。
3. 取消、失败、no-FFmpeg gate 等 terminal path 必须发送 `cancellable=false` 的 terminal progress，让 UI 清理导出状态。
4. 前端进度条测试必须模拟中间 `export-progress` 事件，而不能只验证最终 summary。

#### BUG-0012_1: 第 1 轮人工验证结果

✅ 验证通过，已解决

#### BUG-0012_2: 第 2 轮整改（terminal error UI 边界）

✅ 第二轮人工验证通过，已解决

**整改内容**：terminal export error 写入 `beautifyError` 确保错误显示。当 `!payload.cancellable && payload.error` 时，除了设置 `isExporting=false`，还设置 `beautifyError=payload.error`。

**新增预防规则**：

11. terminal progress error 必须被 UI 展示。
12. no-FFmpeg gate、取消、失败都必须让 UI 清理 exporting 状态并显示明确结果。

---

### BUG-005: 音频捕获失败

**当前状态**：已修复（第 20-26 节整改）+ 人工验证通过。

**根因**（多层叠加）：

1. **CPAL 配置协商**：`cpal_microphone.rs` 把 UI 目标格式（48kHz/2ch）当作硬件 stream config，但真实设备只支持 24kHz/1ch。
2. **writer audio timeline merge**：`ffmpeg_writer.rs` 的音频时间轴合并逻辑未覆盖 gap/overlap/out-of-order 场景。
3. **AudioMixer::to_stereo()**：多声道输入（>2ch）未正确截取前两个通道。
4. **AudioSynchronizer**：旧的 per-chunk 配对导致 system/mic 双写同一时间轴。
5. **audible_min_rms 硬门控**（BUG-005_2）：`audible_min_rms=0.015` 作为硬失败阈值，但 global decoded RMS 被 silence padding 稀释，真实有声录制的 RMS 可能远低于该值。

**修复内容**：

1. **CPAL 配置协商**（第 20 节 R2）：`cpal_microphone.rs` 使用 `device.default_input_config()` 获取真实设备配置，不再把 UI 目标格式当作硬件 stream config。
2. **writer audio timeline merge**（第 21 节 R3）：`ffmpeg_writer.rs` 的音频时间轴合并逻辑已重写。
3. **AudioMixer::to_stereo()**（第 21 节 R4）：多声道输入（>2ch）正确截取前两个通道。
4. **RequestedAudioArtifactContract**（第 24 节 R1）：新增 `RequestedAudioContract` 结构体和 `validate_source_artifact_with_audio_contract()` / `validate_export_artifact_with_audio_contract()`。录制结束后自动解码 artifact 并检查 decoded RMS/peak，当请求了音频但 artifact 静音时返回错误。
5. **AudioSynchronizer window merger**（第 24 节 R2）：重构为固定 20ms 窗口合并器，同一窗口只输出一个 mixed chunk，彻底解决 system/mic 双写同一时间轴问题。
6. **WriterDiagnostics**（第 24 节 R3）：新增 `WriterDiagnostics` 结构体，区分 queued/appended/discarded/trimmed/encoded，`RecordingResult` 携带写入器诊断。
7. **finish() non-blocking**（第 24 节 R4）：`finish()` 改为 `try_send(Flush)` + bounded retry，不再无限阻塞。
8. **strict synthetic artifact helper**（第 24 节 R5）：新增 `create_synthetic_source_artifact_strict()`，任何 push 失败立即返回错误。音频内容测试（RMS/peak 验证）使用 strict helper，backpressure 测试保留 tolerant helper。
9. **麦克风设备选择和蓝牙兼容提示**（第 24 节 R6）：后端新增 `list_microphone_devices` 命令返回设备列表和蓝牙检测。前端新增麦克风设备选择器，对蓝牙麦克风显示 HFP profile 兼容性警告。
10. **audible_min_rms 降级为 warning**（第 26 节 Phase A）：`validate_source_artifact_with_audio_contract()` 和 `validate_export_artifact_with_audio_contract()` 中的 `audible_min_rms` 检查从 `Err(AppError)` 降级为 `eprintln!` 警告。`min_rms/min_peak` 保持硬失败用于防止 truly silent artifact。根因：global decoded RMS 被 silence padding 稀释，真实有声录制的 RMS 可能远低于 0.015。
11. **source-aware contract 去除 RMS 门控**（第 26 节 Phase C）：`validate_source_aware_audio_contract()` 的 presence 检查改为基于 chunk/window/frame 计数，不再以 `system_rms_max > 0.001` 作为前置条件。低音量内容不等于 source 缺失。writer discard 检查对称覆盖 system 和 mic。
12. **low-RMS regression tests**（第 26 节 Phase B）：新增 `create_synthetic_source_artifact_strict_with_amplitude()` helper 和 3 个回归测试，覆盖 RMS ≈ 0.010 的非静音 artifact 通过验证、近静音 artifact 仍被拒绝的场景。

**预防规则**：

1. 麦克风硬件 stream config 必须来自设备 `default_input_config()` 或 supported config；UI 目标格式只能作为 mixer/output target，不能直接当作 CPAL 硬件配置。
2. 音频 chunk 必须携带真实 `timestamp/sample_rate/channels`；统一 48kHz/stereo 只能发生在 mixer 或 writer timeline 层。
3. `AudioMixer` 入口必须校验 `channels > 0`、`sample_rate > 0`、`samples.len() % channels == 0`，不能信任底层捕获元数据。
4. system/mic 同步器必须 source-aware：同一固定时间窗口最多输出一个 mixed chunk，不能让先到的单源 chunk 独占时间轴并吞掉后到源。
5. 同步器窗口必须保留 per-source metadata；system 与 mic 不得共用一份 `sample_rate/channels`。
6. 双源请求但仅一路先到时，必须有 source-start grace；grace 和 stall timeout 都需要测试覆盖，避免初始慢源被误判为缺失。
7. writer 处理 audio gap 时，padding silence 后必须继续 append 当前真实 chunk；gap padding 不能替代 chunk append。
8. writer 必须同时覆盖 first-gap、middle-gap、tail-gap、full-overlap、partial-overlap、out-of-order 六类时间轴场景，并用 decoded RMS/peak 验证真实 PCM 未丢失。
9. writer partial-overlap append 后必须立即 drain AAC buffer，不能跳过 drain 直接进入下一轮或等到 finish。
10. `audio_pts` 只能作为编码器单调 PTS 计数器；timeline cursor 只用于 gap/overlap 决策，两者不能混用。
11. writer diagnostics 必须区分 queued、received、appended、discarded、trimmed、real PCM frames、silence padding、encoded AAC frames；不能用 queued 或 AAC frame 数冒充 artifact 有声证据。
12. `generated_silent_track` 必须来自 writer diagnostics，不能通过 `mixed_audio_chunk_count == 0` 推断，因为 silent packet count 会覆盖该计数。
13. capture channel drop count、writer queue full count、writer push failure count 必须进入 diagnostics；音频 drop 不能静默。
14. FFmpeg writer queue 必须使用 non-blocking send，consumer loop 必须限制每轮 video drain batch，避免编码压力阻塞捕获主链路或饿死音频。
15. 录制停止/finalize 的 bounded 语义必须覆盖 flush send 和 worker join；只在 `join()` 返回后记录耗时不等于 bounded join。
16. CPAL lazy timestamp offset 不能用 `0` 同时表示“未初始化”和“真实 offset 为 0”；真实 0 offset 是合法值，必须用单独 initialized flag 或 sentinel。
17. 蓝牙耳机麦克风会触发 macOS HFP profile 切换；stop 顺序必须 mic first，且需要显式 pause/drop stream、短等待和下一轮重建 mic capture 实例。
18. 麦克风 UI 电平只能证明 capture callback 有输入，不能作为 source artifact 或 export artifact 有声成功证据。
19. 请求录制音频源时，必须建立 source/export artifact contract：至少检查 stream presence、decoded sample count、decoded RMS/peak、source-aware before-writer 计数和 writer discard ratio。
20. 真实设备 manual gate 必须覆盖：只系统音频、只麦克风、系统+麦克风、蓝牙麦克风、低音量输入、停止后蓝牙音质恢复、source 与 export 都可听。
21. 蓝牙麦克风 stop 必须返回结构化 `CpalMicrophoneStopDiagnostics`（pause_attempted/pause_ok/stream_dropped/callbacks_after_stop/stop_wait_ms），不能只靠 eprintln。
22. 少量音频 chunk drop（<10% drop ratio）只能进入 diagnostics warning，不能直接导致 stop 失败；hard fail 需基于 drop ratio 阈值或 artifact contract 失败。
23. writer worker 和 consumer thread 的 join 必须有 bounded timeout（worker 10s、consumer 15s），超时返回结构化错误而非无限阻塞。
24. 音频 channel drop logging 必须标识 source（system/mic/video），不能只记录数量。
25. CPAL callback 中的 drop 不能用 `let _` 静默忽略，必须配合 channel 层 source logging。
26. 录制 stop 路径必须使用 RAII guard 或等效机制，确保 panic 时资源仍被释放。
27. stop-during-startup 场景必须有测试覆盖：stop_flag 在 consumer 启动前设置。
28. writer worker 和 consumer thread timeout 后绝不能调用无界 join()；timeout 必须直接返回结构化错误并继续 cleanup。
29. drop ratio 分母必须使用 received + dropped（attempted total），不能只用 received。
30. mic stop diagnostics 必须在重建 capture 前写入 RecordingDiagnostics，不能依赖 capture 内部字段。
31. writer diagnostics 必须区分 per-source（system/mic）的 chunks received，不能只用 aggregate。
32. per-source writer counter 必须在 push_audio() 成功后递增，不能在 push 前递增。
33. RecordingResult 必须携带 RecordingDiagnostics，不能仅靠 eprintln 暴露诊断信息。
34. stop 失败时 RecordingResult 必须携带 finalization_errors 和完整 diagnostics，不能将 diagnostics 丢失在 Err(String) 中。
35. stop 命令必须返回结构化 `StopRecordingResponse`（含 `result` + `failed`），不能把 hard finalize failure 包装成 command success。`failed=true` 时前端必须进入 failed UI，不能进入 preview。
36. timeout 回归测试必须调用生产 timeout helper（`join_worker_with_timeout` / `receive_consumer_output_with_timeout`），不能只验证标准库 `recv_timeout()` 行为。测试用 parked thread + never-send channel 触发真实 timeout 分支，断言 elapsed 远小于生产 timeout 且 handle 被 detach。

---

### BUG-005_2: 录制音频 contract 验证失败（audible_min_rms false positive）

**当前状态**：已修复（第 26 节整改）+ 人工验证通过。

**根因**：Section 10 把 `audible_min_rms=0.015` 从"可听性诊断阈值"升级成了 source/export artifact 的硬失败阈值。真实录制 artifact 的 aggregate decoded RMS 为 `0.010835`，已经高于非静音阈值 `min_rms=0.003`，capture/writer 诊断均证明音频真实进入了 writer；但它低于 `audible_min_rms=0.015`，因此 stop 阶段被误判为录制失败。

global decoded RMS 被 silence padding 稀释（leading gaps、middle gaps、tail padding to video end），真实有声内容的 RMS 可能远低于 per-chunk peak RMS。

**修复内容**：

1. `audible_min_rms` 从硬失败降级为 `eprintln!` 诊断警告。
2. `min_rms + min_peak` 联合判断保持硬失败，用于防止 truly silent artifact。
3. source-aware contract 去除 RMS 门控，presence 检查基于 chunk/window/frame 计数。
4. writer discard 检查对称覆盖 system 和 mic。
5. 新增 `create_synthetic_source_artifact_strict_with_amplitude()` helper 和 3 个 low-RMS 回归测试。

**预防规则**：

1. `audible_min_rms` 只能作为 warning/diagnostic/manual-gate 阈值，不能作为 source/export artifact validation 的硬失败阈值。
2. global decoded RMS 会被 leading/middle/tail silence padding 稀释；不能把 aggregate RMS 直接等同于用户听到的响度。
3. artifact 静音硬失败只能使用保守 Level 1 contract：`rms < min_rms && peak < min_peak`。
4. 只要 `rms >= min_rms` 或 `peak >= min_peak`，artifact 就不能因为低于 `audible_min_rms` 而失败，只能记录 warning。
5. source-aware presence contract 不能以 RMS 作为前置门控条件；低音量请求源的 chunks/windows/frames 非零即视为 source 存在。
6. before-writer diagnostics 必须记录每个源的 windows、frames、RMS；BUG 定位时优先用这些字段区分 capture 缺失、synchronizer 丢失、writer 丢失和 artifact validation 误判。
7. writer discard ratio 检查必须对称覆盖 system 和 mic，不能只保护其中一路。
8. low-RMS regression 必须同时覆盖 source artifact 和 export artifact：`RMS≈0.010~0.011` 应通过，near-silent artifact 仍应失败。
9. 修改任何 RMS/peak 阈值前，必须先查看真实设备日志和 decoded stats，不能只根据合成测试的响度调阈值。
10. 报错文案必须区分“近乎静音 hard failure”和“低于建议可听阈值 warning”，避免把诊断 warning 展示成录制失败。

---

### BUG-009: 选择系统默认麦克风会导致录制失败

**当前状态**：已修复（第 25 节整改）。

**根因**：`FfmpegRecordingWriter` 音频 timeline gap 分支实现错误。当 `target_sample > audio_timeline_cursor` 时，writer 只补齐 gap silence 并推进 cursor 到 chunk 起点，没有追加当前 audio chunk 的真实 samples，也没有把 cursor 推进到 chunk 末尾。真实设备录制的首个音频 chunk 通常带有非零 timestamp（因为 CPAL callback 有延迟），因此大量非静音 PCM 被替换为静音 AAC frame。

**修复内容**：

1. **writer gap 分支修复**：gap 分支补齐静音后必须继续 append 当前 chunk 的真实 PCM 样本，cursor 推进到 chunk 结束位置。提取 `append_audio_chunk_to_timeline()` helper 统一处理 gap/overlap/contiguous 三种情况。
2. **WriterDiagnostics 语义修正**：新增 `audio_real_frames_appended`、`audio_silence_frames_padded`、`audio_real_rms_max_before_encode`、`silent_aac_frames_encoded`、`generated_silent_track` 字段，区分真实 PCM append 与 silence padding。
3. **AudioSynchronizer per-source metadata**：`AudioWindow` 改为 `SourceWindowBuffer` 结构，system 和 mic 各自保留 `sample_rate/channels`，避免 48kHz/2ch system 与 48kHz/1ch mic 被套用同一份 metadata。
4. **silent track diagnostics**：`generated_silent_track` 从 writer diagnostics 直接获取，不再通过 `mixed_audio_chunk_count == 0` 推断。

**预防规则**：

1. writer 处理 audio gap 时，padding silence 后必须继续 append 当前真实 chunk；gap padding 不能替代 chunk append。
2. 音频 timeline 单元测试不能只检查 duration，还必须检查 decoded RMS/peak。
3. writer diagnostics 必须区分 real PCM append 与 silence padding。
4. `aac_frames_encoded > 0` 不能作为"artifact 有声"的证据，只能说明 AAC encoder 输出了 frame。
5. synchronizer window 必须保留 per-source metadata，不能把 system/mic 两路 PCM 套用同一份 sample_rate/channels。

---

### BUG-004: 导出视频无法播放（视频 PTS 被压缩到 0.03s）

**现象**：

- 导出视频只有封面第一帧有画面，从第二帧开始黑屏
- 原 20 秒视频导出后变成 2 分多钟
- `ffprobe` 显示 export video stream duration 仅 `0.032552s`，而 audio/container 约 `19s`

**根因**（两层叠加）：

1. **exporter packet rescale 空操作**：`trim_exporter.rs` 中 `enc_pkt.rescale_ts(video_enc_tb, video_enc_tb)` 等于没有转换。`write_header()` 后 MP4 muxer 可能把 output stream time base 改为 `1/15360`，但 PTS `0,1,2...` 仍被当作 `1/30` 单位写入，实际被解释为 `1/15360` 单位 → 501 帧仅 `501/15360 ≈ 0.03s`。
2. **source writer PTS 模型与真实时间脱节**：writer 使用固定帧序号 `0,1,2...` 作为 PTS，但 `duration_secs` 来自最后一帧的真实 timestamp。当帧率不稳定或有丢帧时，source video duration 远短于 audio/container duration。

**修复**：

1. `trim_exporter.rs`：`write_header()` 后读取 muxer 真实 output stream time base，所有 encoded packet 使用 `rescale_ts(enc_tb, out_tb)` 正确转换。
2. `ffmpeg_writer.rs`：视频 PTS 改为基于真实 frame timestamp 转换到 encoder time base，保证 monotonicity。
3. `ffmpeg_common.rs`：`MediaArtifactInspection` 增加 `video_duration_nanos` / `audio_duration_nanos` 字段，validation 检查 video/audio drift。

**预防规则**：

- FFmpeg muxer 写包必须使用 `write_header()` 后的真实 output stream time base，不能假设 encoder time base 等于 muxer time base
- artifact validation 必须检查 per-stream duration（video 和 audio），不能只看 container duration
- writer 不得只用 frame count 伪造真实录制时间轴，必须处理 frame sparsity/drop
- playable export manual gate 必须包含 ffprobe stream-level duration/fps 检查

---

### BUG-006: 系统音频稀疏时间轴导致 source writer duration drift

**现象**：

- 只开启系统音频，并且系统存在音频播放时（比如打开音乐播放器播放音乐），点击开始录制
- `录制中界面` 报错：
  - `完成录制文件失败：录制写入器完成失败：写入录制文件失败：录制视频/音频时长偏差过大：视频 22600ms，音频 12821ms，偏差 9779ms`
- 终端日志如下：

```
     Running `target/debug/luzhi`
[libx264 @ 0x713711c00] using cpu capabilities: ARMv8 NEON DotProd
[libx264 @ 0x713711c00] profile Constrained Baseline, level 4.0, 4:2:0, 8-bit
[aac @ 0x713712300] Qavg: 993.605
[libx264 @ 0x713711c00] frame I:3     Avg QP: 9.00  size:392678
[libx264 @ 0x713711c00] frame P:676   Avg QP: 1.82  size: 23728
[libx264 @ 0x713711c00] mb I  I16..4: 100.0%  0.0%  0.0%
[libx264 @ 0x713711c00] mb P  I16..4:  2.3%  0.0%  0.0%  P16..4: 17.8%  0.0%  0.0%  0.0%  0.0%    skip:79.8%
[libx264 @ 0x713711c00] final ratefactor: 6.86
[libx264 @ 0x713711c00] coded y,uvDC,uvAC intra: 34.7% 18.1% 17.0% inter: 7.8% 2.6% 2.4%
[libx264 @ 0x713711c00] i16 v,h,dc,p: 59% 38%  2%  2%
[libx264 @ 0x713711c00] i8c dc,h,v,p: 77% 14%  8%  1%
[libx264 @ 0x713711c00] kb/s:6085.99
录制写入器完成失败: 写入录制文件失败：录制视频/音频时长偏差过大：视频 22600ms，音频 12821ms，偏差 9779ms
```

**根因**：

系统音频捕获天然可能是稀疏的：用户开始录制后几秒才播放声音，中途暂停播放，或结束前没有声音。同步器和 mixer 已经保留 timestamp，但 writer 把 timestamp 丢了，最终 AAC stream duration 只等于"有声样本总长度"，不是"录制时间轴长度"。

**预防规则**：

- 音频输入 chunk 必须携带真实 sample_rate/channels/timestamp；统一输出格式只能在 mixer/writer timeline 层完成
- writer 必须尊重 mixed audio timestamp；对前导 gap、中间 gap、尾部 gap 写入 silence，不能把稀疏音频压缩成连续短音轨

---

### BUG-007: 导出的视频没有美化（默认美化开启时无光标）

**现象**：

- 默认美化开启（`cursor_magnification=true, cursor_smoothing=true`）时，录制会隐藏系统光标
- FFmpeg exporter 未应用 `effect_timeline_path` 绘制光标效果
- 导出视频无光标、无美化，违背 MVP "录屏 + AI 自动美化 + 一键导出" 核心目标

**根因**：

1. `build_cursor_effect_timeline()` 失败时静默降级为 `effect_timeline_path: None`
2. `FfmpegTrimExporter::export()` 从未读取 `effect_timeline_path` JSON
3. 没有 cursor overlay compositor 在导出帧上绘制光标

**修复**：

1. 新增 `cursor_overlay.rs` 模块：`CursorOverlayRenderer` 加载 EffectTimeline，映射坐标到输出帧空间（支持 FitWithBars 和 CenterCrop），在 YUV420P 帧上绘制光标（白点 + 暗边框 + 点击放大环）
2. `trim_exporter.rs`：导出时加载 effect timeline 并在每帧编码前叠加 cursor overlay
3. `lib.rs`：当美化开启（`cursor_magnification || cursor_smoothing`）时，cursor timeline 构建失败必须阻断导出，不再静默降级
4. `ffmpeg_writer.rs`：重构为 worker-backed 架构，编码在独立线程执行，bounded channel 提供背压

**预防规则**：

- 当 raw system cursor hidden 时，exporter 必须应用 cursor effect timeline，或导出必须失败，不得静默产出无光标视频
- cursor effect timeline 不能只作为 request 字段存在，exporter 必须实际读取并应用
- 默认美化开关、capture `show_system_cursor`、export compositor 三者必须作为端到端 contract 测试
- UI 不能把"可播放导出成功"文案等同于"美化导出成功"

---

### BUG-008: cursor overlay scale/radius 数值溢出导致 export panic

**现象**：

- 如果当前系统没有音频输出时，录制能成功，但是`美化界面`导出会报错：
  - `导出失败，请重试或检查录制素材。`
- 终端日志如下：

```
     Running `target/debug/luzhi`
[libx264 @ 0xab32e4700] using cpu capabilities: ARMv8 NEON DotProd
[libx264 @ 0xab32e4700] profile Constrained Baseline, level 4.0, 4:2:0, 8-bit
[aac @ 0xab32e4e00] Qavg: 65536.000
[libx264 @ 0xab32e4700] frame I:2     Avg QP:17.00  size:288984
[libx264 @ 0xab32e4700] frame P:384   Avg QP: 5.91  size: 33077
[libx264 @ 0xab32e4700] mb I  I16..4: 100.0%  0.0%  0.0%
[libx264 @ 0xab32e4700] mb P  I16..4:  4.2%  0.0%  0.0%  P16..4: 17.9%  0.0%  0.0%  0.0%  0.0%    skip:77.8%
[libx264 @ 0xab32e4700] final ratefactor: 12.31
[libx264 @ 0xab32e4700] coded y,uvDC,uvAC intra: 40.9% 17.2% 14.0% inter: 9.2% 2.6% 2.2%
[libx264 @ 0xab32e4700] i16 v,h,dc,p: 52% 43%  3%  2%
[libx264 @ 0xab32e4700] i8c dc,h,v,p: 78% 14%  7%  1%
[libx264 @ 0xab32e4700] kb/s:7811.50
[libx264 @ 0xab32e6680] using cpu capabilities: ARMv8 NEON DotProd
[libx264 @ 0xab32e6680] profile Constrained Baseline, level 4.0, 4:2:0, 8-bit

thread 'tokio-rt-worker' (7976776) panicked at src/media/cursor_overlay.rs:263:30:
attempt to multiply with overflow
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
[aac @ 0xab32e6300] Qavg: nan
[aac @ 0xab32e6300] 1 frames left in the queue on closing
[libx264 @ 0xab32e6680] final ratefactor: 20.64
```

**根因**：

cursor effect timeline 来自录制时的外部输入和动画计算，不应被视为可信数值。`scale` 可能异常大、非有限值、或由错误时间轴插值得到极端值。当前 rasterizer 用 `i32` 做平方，debug 构建会 panic，release 构建则可能溢出后产生错误绘制。

**预防规则**：

- cursor/effect timeline 来自外部输入，所有 `x/y/scale/timestamp` 参与 rasterization 前必须 finite check 和范围 clamp
- overlay 距离计算不得依赖 debug/release 不同行为；平方和半径计算必须使用足够宽的整数类型或 saturating arithmetic

---

### BUG-001: 录制面板不是浮动小组件，有800x600背景方框

**现象**：主录制控制面板应为浮动小组件，但显示时有一个800x600的不透明背景方框。修复 body CSS 后背景从深色变为白色，但方框仍然存在。

**根因**：Tauri 透明窗口需要**三层独立**都透明，缺一不可：

| 层        | 机制                        | 问题                                                 |
| --------- | --------------------------- | ---------------------------------------------------- |
| NSWindow  | `"transparent": true`       | ✅ 已配置                                            |
| WKWebView | `macos-private-api` feature | ❌ 缺失：macOS WKWebView 独立于 CSS 绘制默认白色背景 |
| CSS       | html/body 无背景色          | ❌ body `bg-background` (#040506) + html 未设透明    |

第一轮修复只解决了 CSS 层 body 的问题，但 WKWebView 的白色背景随即暴露。之前 body `#040506` 覆盖了 WKWebView 的白色，使人误以为只有 CSS 问题。

**修复**：

1. `Cargo.toml`：tauri 添加 `macos-private-api` feature → 通过 KVC 禁用 WKWebView 的 `drawsBackground`
2. `styles.css`：body 移除 `bg-background` + html 添加 `background-color: transparent`
3. 需要全屏背景的视图（processing、error、preview）已在各自根 div 显式设置 `bg-background`

**预防规则**：

- Tauri macOS 透明窗口 = NSWindow + WKWebView + CSS，三层缺一不可
- 排查透明窗口问题必须逐层验证，不能只看 CSS
- `macos-private-api` feature 是 macOS 平台上实现真正透明 WKWebView 的必要条件

---

### BUG-002: 窗口无法拖动

**现象**：所有状态下窗口都无法通过鼠标拖动移动。

**根因**（多层叠加，逐层剥离后才暴露下一层）：

| 层             | 问题                                                                | 详情                                                                                                                                                                                        |
| -------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| ACL 权限       | `capabilities/default.json` 缺少 `core:window:allow-start-dragging` | Tauri 2 的 drag.js 在 mousedown 时通过 IPC 调用 Rust 命令 `plugin:window                                                                                                                    | start_dragging`，没有此权限 IPC 被静默拒绝。**这是拖拽完全不工作的致命原因** |
| 拖拽区域标记   | bare `data-tauri-drag-region` 渲染为 `"true"`                       | React 将裸属性渲染为 `"true"`，Tauri drag.js 对 `"true"` 的处理是仅元素自身直接点击触发（`el === composedPath[0]`），子元素区域不触发。面板覆盖大部分区域，实际可拖拽区只剩 `p-8` 32px 窄边 |
| macOS API 限制 | `performWindowDragWithEvent:` 需窗口为焦点窗口                      | 即使权限和属性都正确，底层 macOS API 在窗口未获焦点时静默失败（tauri#11605）。声明式机制对浮动面板窗口不可靠                                                                                |
| 排除逻辑过严   | `{false}` 排除 + `attr === 'false'` 检查双重拦截                    | 面板包裹在 `{false}` div 中，`closest('[data-tauri-drag-region]')` 最先匹配到 `{false}` 父元素，handler 拒绝拖拽。面板内容区（非按钮空白处）完全无法拖拽                                    |

**修复**（迭代 4 轮，最终方案：程序化拖拽替代声明式）：

1. **`src-tauri/capabilities/default.json`**：添加 `core:window:allow-start-dragging` 权限 — 解禁 IPC 调用
2. **`src-tauri/tauri.conf.json`**：添加 `"acceptFirstMouse": true` — macOS 首次点击可同时聚焦+拖拽
3. **`src/App.tsx`**：实现程序化拖拽 handler，直接调用 `getCurrentWindow().startDragging()`，绕过不可靠的声明式 drag.js
   - 添加 `data-tauri-drag-region="deep"` 到所有外层容器（idle、recording、preview、processing、error）
   - 拖拽 handler 逻辑：
     - **允许拖拽**：点击在 `[data-tauri-drag-region]` 区域内且不是交互元素
     - **阻止拖拽**：点击目标为 `button, input, select, textarea, a` 或 `role` 为 `button/link/menuitem/tab/checkbox/radio/slider/switch` 或 `contenteditable` / 非 `-1` 的 `tabindex`
   - 使用动态 `import('@tauri-apps/api/window')` 避免测试环境顶层导入报错
   - **不检查 `{false}` 值** — 交互元素检查已足够，`{false}` 标记仅保留供 Tauri 内置 drag.js 作 fallback

**预防规则**：

- **Tauri 2 窗口拖拽必须授予 ACL 权限**：`core:window:allow-start-dragging` 不在 `core:default` 范围内，必须显式添加
- **声明式 `data-tauri-drag-region` 不可靠**：受 macOS 焦点窗口限制，对浮动面板类 app 应优先使用程序化 `startDragging()` API
- **不要用 `{false}` 做区域级拖拽排除**：用交互元素选择器（`closest('button, input, ...')`）精确排除，而非用 `{false}` 阻止整个面板区域。`{false}` 应该是交互元素自身的标记，不是容器的标记
- **每个状态视图都需要拖拽区域**：包括 processing、error 等过渡状态
- **交互元素白名单要覆盖完整**：button, input, select, textarea, a, [role="button"], [role="slider"], [role="switch"], [contenteditable], [tabindex] 等

---

### BUG-003: 点击"开始录制"无法切换到录制状态

**现象**：点击"开始录制"按钮后，界面没有切换到录制状态栏（迷你播放器）。

**根因**：Button 被包裹在 `motion.div` 的 `whileTap={{ scale: 0.99 }}` 中。framer-motion 的 whileTap 会捕获指针事件来实现缩放动画，阻止了点击事件到达内部的 Button 元素。

**修复**：移除 Button 外层的 `motion.div` 包裹，改用 CSS `active:scale-[0.99]` 实现按压反馈效果。

**预防规则**：

- framer-motion 的 `whileTap` 会拦截指针事件，不要将其作为可交互元素（Button、Link 等）的直接父容器。
- 如需在可交互元素上添加按压动画，优先使用 CSS `active:` 伪类，或将 `whileTap` 直接放在元素本身上（如 `motion.button`）。

---

## 延期解决

### 延期-001: 透明区域鼠标点击不穿透

**现象**：窗口视觉透明区域的鼠标点击不会穿透到被覆盖的应用（桌面、其他窗口），点击透明区域不会激活后方应用。

**原因**：Tauri 2 的 `setIgnoreCursorEvents(true)` 是全窗口级开关，开启后整个窗口（包括面板按钮）都无法交互。无像素级点击穿透支持。

**计划方案**（后期实现）：

- 方案 A：自定义 NSWindow `hitTest:` 重写，透明像素处返回 nil（需原生 macOS 代码）
- 方案 B：主窗口忽略鼠标事件 + 独立子窗口承载控制面板
- 方案 C：使用窗口 shape mask 裁剪到面板区域

---
