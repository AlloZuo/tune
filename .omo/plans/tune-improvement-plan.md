# Tune Improvement Plan

## Roadmap Overview

```
Phase 1: Server Adapters  ─── Phase 2: Smarter          ─── Phase 3: Polish
[现在可以做]                  [依赖 Phase 1]               [持续打磨]
                             
Subsonic adapter             Server-side search           Key binding config
Config overlay redesign      Lyrics source plugins        UI touch-ups
Multi-server switch          Smart playlist               Error handling
                             Album art (terminal)         Performance
```

---

## Phase 1 — Server Adapters (预计 3-5 天)

### 1.1 Subsonic/Navidrome Adapter

- `src/subsonic.rs` — 完整的 Subsonic adapter 实现
- 支持认证（username + password）
- `fetch_list()`: 递归获取所有 album → 每个 album 的 songs
- `stream_url()`: 通过 `/rest/stream?id=` 获取流
- Config 增加 `username` / `password` 字段
- 配置界面增加账号密码输入

**复杂度**: ⭐⭐⭐ 中等。Subsonic API 很标准，主要工作是分页和映射字段。

### 1.2 Config Overlay Redesign

当前：
```
╔══ 配置服务器 ══╗
║  类型: 文件闪传    ║
║  URL: ...         ║
╚══════════════════╝
```

目标（根据类型动态显示）：
```
╔══ 配置服务器 ════════════╗
║  类型: Navidrome          ║
║  URL: http://192.168.x:4533 ║
║  用户名: admin             ║
║  密码: ****                ║
╚═══════════════════════════╝
```

- 每个 server type 声明自己需要的字段
- 按 Tab 或左右键切换字段焦点
- 字段写在配置里，不在代码里写死

### 1.3 Server Type Switcher

- 配置界面可用左右键切换 server type
- 切换后字段自动变化
- 一键切换无需重启

---

## Phase 2 — 更智能 (预计 5-7 天)

### 2.1 Server-Side Search

当前：只在本地已加载的列表中搜索。
改进：支持搜索远程服务器（Subsonic `/rest/search3`）。

- 搜索时根据当前 server type 调用对应的搜索 API
- 搜索结果实时展示

### 2.2 Lyrics Source Plugin

当前：仅从音频文件内嵌的 USLT/Vorbis 提取歌词。
改进：支持多个歌词源。

```
trait LyricsProvider {
    async fn search(&self, title: &str, artist: &str) -> Result<Option<Lyrics>>;
}
```

内置源:
- 本地嵌入歌词（当前）
- 未来可加: LrcLib API, 网易云 API

### 2.3 Terminal Album Art

这是一个"有趣但不实用"的功能。可以实现但体验有限。

**方案 A: Sixel/Kitty (仅 Linux/Kitty)**
- 用 `viuer` crate 在播放时显示封面
- 在 now-playing 区域右侧显示
- 问题: Windows 不支持，macOS 仅 iTerm2，布局被破坏

**方案 B: ANSI 字符块**
- 用 `chafa` 或 `halfblocks` 将图片转成彩色字符
- 在所有终端都能显示（16色 或 256色）
- 分辨率极低（80x40 个字符块），只能看个轮廓

**方案 C: 半块字符 (▀ ▄)**
- 用上下半块字符模拟双倍垂直分辨率
- 加上 true color (\x1b[38;2;R;G;Bm) 支持
- 效果最好的纯字符方案

| 方案 | 跨平台 | 美观度 | 实现难度 |
|------|--------|--------|---------|
| Sixel/Kitty | ❌ | ⭐⭐⭐⭐ | ⭐⭐ |
| ANSI 字符块 | ✅ | ⭐⭐ | ⭐ |
| 半块字符 | ✅ | ⭐⭐⭐ | ⭐⭐ |

**结论**: 半块字符方案是唯一值得做的 —— 在所有终端都能工作，效果够看个封面。
但需要 Ratatui 不支持（图片覆盖），需要直接在终端层绘制，和 Ratatui 的布局会有冲突。

---

## Phase 3 — 打磨 (持续)

### 3.1 配置映射表

让 `MusicEntry` 字段可由每个 adapter 配置映射：

```rust
// 文件闪传
absolute_path = response.absoultePath  // serde rename
name           = response.name

// Subsonic
absolute_path = song.id
name           = song.title
artist         = song.artist
```

### 3.2 Key Binding 可配置

- 默认配置在代码里
- 用户可用配置文件覆盖
- 避免硬编码

### 3.3 播放队列改进

- 可视化播放队列（Q 键查看）
- 拖拽排序（需要鼠标支持）
- 清空/保存队列

### 3.4 杂项

- 播放历史记录
- 评分系统 (⭐)
- 统计数据（播放次数、总时长）
- 命令补全（类似 vim 的 `:command` 模式）

---

## Timeline 预估

| Phase | 工作量 | 优先级 |
|-------|--------|--------|
| Phase 1.1 Subsonic adapter | 2-3 天 | 🔴 高 |
| Phase 1.2 Config redesign | 1 天 | 🔴 高 |
| Phase 1.3 Type switcher | 半天 | 🟡 中 |
| Phase 2.1 Server search | 1 天 | 🟡 中 |
| Phase 2.2 Lyrics plugins | 2-3 天 | 🟢 低 |
| Phase 2.3 Album art | 1-2 天 | 🟢 低（实验性） |
| Phase 3 Polish | 持续 | 🟢 低 |

## 我的建议

1. 先做 **Phase 1**（Subsonic adapter + 配置界面）—— 这是功能扩展的基础
2. **Album art** 可以当作娱乐项目玩一下，但不要花太多精力
3. **Lyrics plugins** 和 **Server search** 看实际使用中是否需要
4. **Polish** 永远可以做，但也永远没有尽头——适可而止
