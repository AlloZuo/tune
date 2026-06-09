# ♪ Tune — 终端音乐播放器 / Terminal Music Player

[English](#english) · [中文](#中文)

---

# 中文

Tune 是一个运行在终端中的流式音乐播放器，基于 [Ratatui](https://github.com/ratatui/ratatui) 构建。支持多个音乐后端同时接入、本地文件夹播放、歌词同步、歌单管理。

## 功能

- **多服务器** — 同时接入文件闪传、Navidrome 等多个后端，列表自动合并
- **本地播放** — 直接扫描本地文件夹，无需服务器
- **流式播放** — 从远程服务器缓冲音频，边下边播
- **歌词同步** — 自动提取嵌入的 LRC 歌词，支持原词/翻译分栏显示
- **歌单管理** — 创建、删除歌单，从列表添加歌曲
- **播放模式** — 顺序播放、单曲循环、随机洗牌
- **搜索过滤** — 按歌曲名或艺术家实时筛选
- **进度控制** — 快进/快退、音量调节
- **跳转到当前播放** — 一键定位正在播放的歌曲
- **播放记忆** — 自动切歌时不干扰浏览光标
- **错误日志** — 错误信息写入 `tune.log`，方便排查

## 截图

```
╭ I 'll go along with you, ────────────────────────────────────────────────────────────────╮╭──────────╮
│██████████████████ ⏸ Cara Dillon - Cara Dillon - Craigie Hill.mp3  [01:08/04:44]  [顺序]  ││音量: 85% │
╰ 我还要与你在一起 ────────────────────────────────────────────────────────────────────────╯╰──────────╯
```

## 依赖

- Rust 2024 edition（需要较新的 rustc）

## 快速开始

```bash
# 克隆并构建
git clone https://github.com/AlloZuo/tune && cd tune
cargo build --release

# 运行（首次启动会自动弹出配置界面）
cargo run --release

# 按 R 键管理服务器列表，按 r 刷新音乐列表
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
| `R` | 管理服务器（添加/编辑/删除/停用） |
| `l` | 歌单管理 / 返回 |
| `a` | 加入歌单 |
| `c` | 创建歌单 |
| `d` | 删除歌单 / 移出 |
| `h` / `?` | 帮助 |
| `q` / `Esc` | 退出 / 返回 |

### 配置服务器（按 R）

| 按键 | 功能 |
|------|------|
| `↑` / `↓` | 选择服务器 |
| `Enter` | 编辑选中服务器 |
| `Space` | 停用/启用服务器 |
| `a` | 添加新服务器 |
| `d` | 删除服务器 |
| `Tab` / `Shift+Tab` | 切换输入字段 |
| `←` / `→` | 切换服务器类型 |

## 播放模式

- **顺序播放** — 播完列表最后一首后停止
- **单曲循环** — 无限重复当前歌曲
- **随机播放** — 随机顺序播放，列表播完后重新洗牌

## 歌词格式

支持嵌入在 MP3/FLAC 中的 LRC 歌词。对于带翻译的歌词，支持两种格式：

- **同一行**：`英文歌词\u{2009}中文翻译`（thin space 分隔）
- **同一时间戳**：`[00:05.00]英文\n[00:05.00]中文`

原词显示在上边框，翻译显示在下边框。

## 服务器适配器

Tune 使用适配器模式支持不同的音乐后端。每种后端只需实现 `MusicServer` trait：

```rust
#[async_trait]
pub trait MusicServer: Send + Sync {
    fn name(&self) -> &str;
    fn base_url(&self) -> &str;
    fn features(&self) -> ServerFeatures;
    async fn fetch_list(&self) -> Result<Vec<MusicEntry>>;
    fn stream_url(&self, music: &MusicEntry) -> String;
    async fn fetch_audio(&self, music: &MusicEntry) -> Result<Vec<u8>>;
    async fn search(&self, _query: &str) -> Result<Vec<MusicEntry>>;
    fn cover_url(&self, _music: &MusicEntry) -> Option<String>;
    async fn fetch_lyrics(&self, _music: &MusicEntry) -> Option<Lyrics>;
}
```

### 内置后端

| 类型标识 | 名称 | 说明 |
|----------|------|------|
| `file-transfer` | 文件闪传 | 通过 `/musicsV2` 和 `/file?path=` API 提供音乐 |
| `navidrome` / `subsonic` | Navidrome | Subsonic API 兼容服务器（Navidrome、Airsonic 等） |
| `local` | 本地文件夹 | 扫描本地目录，直接读取音频文件 |

### 配置文件

首次运行后生成 `tune_config.json`（支持多服务器，数组格式）：

```json
[
  {
    "name": "我的服务器",
    "server_url": "http://192.168.1.x:2333",
    "server_type": "file-transfer",
    "username": "",
    "password": ""
  }
]
```

按 `R` 进入服务器管理界面，支持添加、编辑、删除、停用任意数量的服务器。

### 扩展

要添加新的后端，只需：

1. 新建文件实现 `MusicServer` trait
2. 在 `api.rs` 的 `create_server()` 工厂中注册

## 项目结构

```
tune/
├── src/
│   ├── main.rs      # 主循环、事件处理、自动切歌
│   ├── ui.rs        # TUI 渲染（~1450 行）
│   ├── api.rs       # 服务器适配器 trait、工厂、ServerPool
│   ├── player.rs    # 音频播放引擎
│   ├── lyrics.rs    # LRC 解析与歌词提取
│   ├── navidrome.rs # Navidrome (Subsonic API) 适配器
│   ├── local.rs     # 本地文件夹适配器
│   ├── store.rs     # 配置与歌单持久化
│   └── log.rs       # 文件日志（tune.log）
├── Cargo.toml
└── README.md
```

## 技术栈

- **终端 UI** — Ratatui + Crossterm
- **音频** — Rodio (Symphonia 解码)
- **歌词提取** — Lofty (ID3v2 USLT / Vorbis Comments)
- **异步** — Tokio + async-trait
- **网络** — Reqwest
- **序列化** — Serde + Chrono

## 许可

MIT

---

# English

**Tune** is a terminal-based streaming music player built with [Ratatui](https://github.com/ratatui/ratatui). It supports multiple backends simultaneously (remote servers + local files), playlist management, synced lyrics, and multiple play modes.

## Features

- **Multi-server** — connect to file-transfer, Navidrome, and local folders at the same time
- **Local playback** — scan a local directory, no server needed
- **Streaming playback** — buffers audio from remote servers on-the-fly
- **Synced lyrics** — automatically extracts embedded LRC lyrics; supports original + translation display
- **Playlist management** — create, delete playlists; add/remove songs
- **Play modes** — sequential, single-repeat, shuffle
- **Search** — real-time filter by song name or artist
- **Seek & volume** — forward/backward seek, volume control
- **Jump to playing** — one key to locate the currently playing song
- **Non-intrusive auto-next** — cursor stays put when tracks advance
- **Error logging** — errors written to `tune.log` for debugging

## Screenshot

```
╭ I 'll go along with you, ────────────────────────────────────────────────────────────────╮╭──────────╮
│██████████████████ ⏸ Cara Dillon - Cara Dillon - Craigie Hill.mp3  [01:08/04:44]  [顺序]  ││Volume:85%│
╰ 我还要与你在一起 ────────────────────────────────────────────────────────────────────────╯╰──────────╯
```

## Prerequisites

- Rust 2024 edition (requires a recent rustc)

## Getting Started

```bash
git clone https://github.com/AlloZuo/tune && cd tune
cargo build --release

# Run (the config screen will pop up on first launch)
cargo run --release

# Press R to manage servers, r to refresh the music list
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
| `R` | Manage servers (add/edit/delete/disable) |
| `l` | Playlist management / back |
| `a` | Add to playlist |
| `c` | Create playlist |
| `d` | Delete playlist / remove song |
| `h` / `?` | Help |
| `q` / `Esc` | Quit / back |

### Config overlay (press R)

| Key | Action |
|-----|--------|
| `↑` / `↓` | Select server |
| `Enter` | Edit selected server |
| `Space` | Toggle disable/enable |
| `a` | Add new server |
| `d` | Delete server |
| `Tab` / `Shift+Tab` | Switch fields |
| `←` / `→` | Toggle server type |

## Play Modes

- **Sequential** — plays through the list, stops at the end
- **Single Repeat** — repeats the current track forever
- **Shuffle** — random order; re-shuffles when exhausted

## Lyrics Format

Supports LRC lyrics embedded in MP3/FLAC. Translations are handled in two ways:

- **On the same line**: `English lyric\u{2009}Translation` (thin space separator)
- **Same timestamp, separate lines**: `[00:05.00]English\n[00:05.00]Translation`

Original text appears on the top border of the playback bar, translation on the bottom border.

## Server Adapters

Tune uses the adapter pattern to support different backends. Each backend implements the `MusicServer` trait:

```rust
#[async_trait]
pub trait MusicServer: Send + Sync {
    fn name(&self) -> &str;
    fn base_url(&self) -> &str;
    fn features(&self) -> ServerFeatures;
    async fn fetch_list(&self) -> Result<Vec<MusicEntry>>;
    fn stream_url(&self, music: &MusicEntry) -> String;
    async fn fetch_audio(&self, music: &MusicEntry) -> Result<Vec<u8>>;
    async fn search(&self, _query: &str) -> Result<Vec<MusicEntry>>;
    fn cover_url(&self, _music: &MusicEntry) -> Option<String>;
    async fn fetch_lyrics(&self, _music: &MusicEntry) -> Option<Lyrics>;
}
```

### Built-in Adapters

| Type ID | Name | Description |
|---------|------|-------------|
| `file-transfer` | 文件闪传 | Serves music via `/musicsV2` and `/file?path=` endpoints |
| `navidrome` / `subsonic` | Navidrome | Subsonic API compatible servers (Navidrome, Airsonic, etc.) |
| `local` | Local folder | Scan a local directory, read audio files directly |

### Config File

Created on first launch (`tune_config.json`, array format with multi-server support):

```json
[
  {
    "name": "my-server",
    "server_url": "http://192.168.1.x:2333",
    "server_type": "file-transfer",
    "username": "",
    "password": ""
  }
]
```

Press `R` to open the server management overlay — add, edit, delete, or disable any number of servers.

### Extending

To add a new backend:

1. Create a file implementing `MusicServer`
2. Register it in `api.rs`'s `create_server()` factory

## Project Layout

```
tune/
├── src/
│   ├── main.rs      # Main loop, event dispatch, auto-advance
│   ├── ui.rs        # TUI rendering (~1450 lines)
│   ├── api.rs       # Server adapter trait, factory, ServerPool
│   ├── player.rs    # Audio playback engine
│   ├── lyrics.rs    # LRC parser & lyric extraction
│   ├── navidrome.rs # Navidrome (Subsonic API) adapter
│   ├── local.rs     # Local folder adapter
│   ├── store.rs     # Config & playlist persistence
│   └── log.rs       # File logger (tune.log)
├── Cargo.toml
└── README.md
```

## Tech Stack

- **TUI** — Ratatui + Crossterm
- **Audio** — Rodio (Symphonia decoder)
- **Lyrics** — Lofty (ID3v2 USLT / Vorbis Comments)
- **Async** — Tokio + async-trait
- **HTTP** — Reqwest
- **Serialization** — Serde + Chrono

## License

MIT
