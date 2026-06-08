# ♪ Tune — 终端音乐播放器 / Terminal Music Player

[English](#english) · [中文](#中文)

---

# 中文

Tune 是一个运行在终端中的流式音乐播放器，基于 [Ratatui](https://github.com/ratatui/ratatui) 构建。它从自建音乐服务器获取数据，支持歌单管理、歌词显示、多种播放模式。

## 功能

- **流式播放** — 从远程服务器缓冲音频，边下边播
- **歌词同步** — 自动提取嵌入的 LRC 歌词，支持原词/翻译分栏显示
- **歌单管理** — 创建、删除歌单，从列表添加歌曲
- **播放模式** — 顺序播放、单曲循环、随机洗牌
- **搜索过滤** — 按歌曲名或艺术家实时筛选
- **进度控制** — 快进/快退、音量调节
- **跳转到当前播放** — 一键定位正在播放的歌曲
- **播放记忆** — 自动切歌时不干扰浏览光标

## 截图

```
╭ I 'll go along with you, ────────────────────────────────────────────────────────────────╮╭──────────╮
│██████████████████ ⏸ Cara Dillon - Cara Dillon - Craigie Hill.mp3  [01:08/04:44]  [顺序]  ││音量: 85% │
╰ 我还要与你在一起 ────────────────────────────────────────────────────────────────────────╯╰──────────╯
```

## 依赖

- **文件闪传** ([https://www.xiaolifaa.com/](https://www.xiaolifaa.com/)) — 手机端 App，提供 `/musicsV2` 和 `/file?path=` API
- Rust 2024 edition

## 快速开始

```bash
# 克隆并构建
git clone <repo-url> && cd tune
cargo build --release

# 运行（首次启动会自动弹出配置界面）
cargo run --release

# 按 R 键在运行时修改服务器地址
```

## 快捷键

| 按键 | 功能 |
|------|------|
| `↑` / `↓` | 上 / 下选择 |
| `PgUp` / `PgDn` | 翻页 |
| `Enter` | 播放选中 |
| `Space` | 暂停 / 继续 |
| `s` | 停止 |
| `←` / `→` | 快退 / 快进 5 秒 |
| `m` | 切换播放模式 |
| `g` | 跳转到正在播放 |
| `+` / `=` | 音量增加 |
| `-` / `_` | 音量减少 |
| `/` | 搜索 |
| `r` | 刷新音乐列表 |
| `R` | 配置服务器地址 |
| `l` | 歌单管理 / 返回 |
| `a` | 加入歌单 |
| `c` | 创建歌单 |
| `d` | 删除歌单 / 移出 |
| `h` / `?` | 帮助 |
| `q` / `Esc` | 退出 / 返回 |

## 播放模式

- **顺序播放** — 播完列表最后一首后停止
- **单曲循环** — 无限重复当前歌曲
- **随机播放** — 随机顺序播放，列表播完后重新洗牌

## 歌词格式

支持嵌入在 MP3/FLAC 中的 LRC 歌词。对于带翻译的歌词，支持两种格式：

- **同一行**：`英文歌词\u{2009}中文翻译`（thin space 分隔）
- **同一时间戳**：`[00:05.00]英文\n[00:05.00]中文`

原词显示在上边框，翻译显示在下边框。

## 项目结构

```
tune/
├── src/
│   ├── main.rs      # 主循环、事件处理、自动切歌
│   ├── ui.rs        # TUI 渲染（~1200 行）
│   ├── api.rs       # 服务器 API 客户端
│   ├── player.rs    # 音频播放引擎
│   ├── lyrics.rs    # LRC 解析与歌词提取
│   └── store.rs     # 配置与歌单持久化
├── Cargo.toml
└── README.md
```

## 技术栈

- **终端 UI** — Ratatui + Crossterm
- **音频** — Rodio (Symphonia 解码)
- **歌词提取** — Lofty (ID3v2 USLT / Vorbis Comments)
- **异步** — Tokio
- **网络** — Reqwest
- **序列化** — Serde

## 许可

MIT

---

# English

**Tune** is a terminal-based streaming music player built with [Ratatui](https://github.com/ratatui/ratatui). It fetches music from a self-hosted server and provides a full-featured TUI experience.

## Features

- **Streaming playback** — buffers audio from a remote server on-the-fly
- **Synced lyrics** — automatically extracts embedded LRC lyrics; supports original + translation display
- **Playlist management** — create, delete playlists; add/remove songs
- **Play modes** — sequential, single-repeat, shuffle
- **Search** — real-time filter by song name or artist
- **Seek & volume** — forward/backward seek, volume control
- **Jump to playing** — one key to locate the currently playing song
- **Non-intrusive auto-next** — cursor stays put when tracks advance

## Screenshot

```
╭ I 'll go along with you, ────────────────────────────────────────────────────────────────╮╭──────────╮
│██████████████████ ⏸ Cara Dillon - Cara Dillon - Craigie Hill.mp3  [01:08/04:44]  [顺序]  ││Volume:85%│
╰ 我还要与你在一起 ────────────────────────────────────────────────────────────────────────╯╰──────────╯
```

## Prerequisites

- **文件闪传** ([Fast File Transfer](https://www.xiaolifaa.com/)) — a mobile app that exposes a web server with `/musicsV2` and `/file?path=` endpoints for browsing and streaming music
- Rust 2024 edition

## Getting Started

```bash
git clone <repo-url> && cd tune
cargo build --release

# Run (the config screen will pop up on first launch)
cargo run --release

# Press R at runtime to change the server URL
```

## Key Bindings

| Key | Action |
|-----|--------|
| `↑` / `↓` | Navigate up / down |
| `PgUp` / `PgDn` | Page scroll |
| `Enter` | Play selected |
| `Space` | Pause / Resume |
| `s` | Stop |
| `←` / `→` | Seek backward / forward 5s |
| `m` | Cycle play mode |
| `g` | Jump to currently playing |
| `+` / `=` | Volume up |
| `-` / `_` | Volume down |
| `/` | Search |
| `r` | Refresh music list |
| `R` | Configure server URL |
| `l` | Playlist management / back |
| `a` | Add to playlist |
| `c` | Create playlist |
| `d` | Delete playlist / remove song |
| `h` / `?` | Help |
| `q` / `Esc` | Quit / back |

## Play Modes

- **Sequential** — plays through the list, stops at the end
- **Single Repeat** — repeats the current track forever
- **Shuffle** — random order; re-shuffles when exhausted

## Lyrics Format

Supports LRC lyrics embedded in MP3/FLAC. Translations are handled in two ways:

- **On the same line**: `English lyric\u{2009}Translation` (thin space separator)
- **Same timestamp, separate lines**: `[00:05.00]English\n[00:05.00]Translation`

Original text appears on the top border of the playback bar, translation on the bottom border.

## Project Layout

```
tune/
├── src/
│   ├── main.rs      # Main loop, event dispatch, auto-advance
│   ├── ui.rs        # TUI rendering (~1200 lines)
│   ├── api.rs       # Server API client
│   ├── player.rs    # Audio playback engine
│   ├── lyrics.rs    # LRC parser & lyric extraction
│   └── store.rs     # Config & playlist persistence
├── Cargo.toml
└── README.md
```

## Tech Stack

- **TUI** — Ratatui + Crossterm
- **Audio** — Rodio (Symphonia decoder)
- **Lyrics** — Lofty (ID3v2 USLT / Vorbis Comments)
- **Async** — Tokio
- **HTTP** — Reqwest
- **Serialization** — Serde

## License

MIT
