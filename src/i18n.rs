// ── i18n: Chinese / English translation module ──

use std::sync::LazyLock;
use std::sync::Mutex;

static LANG: LazyLock<Mutex<Language>> = LazyLock::new(|| Mutex::new(Language::Zh));

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Language {
    Zh,
    En,
}

impl Language {
    pub fn from_str(s: &str) -> Self {
        match s {
            "en" | "English" => Language::En,
            _ => Language::Zh,
        }
    }

    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            Language::Zh => "zh",
            Language::En => "en",
        }
    }

    #[allow(dead_code)]
    pub fn toggle(self) -> Self {
        match self {
            Language::Zh => Language::En,
            Language::En => Language::Zh,
        }
    }
}

/// Initialise the global language from a config string ("zh" / "en").
/// Can be called multiple times to switch language at runtime.
pub fn init(lang: &str) {
    *LANG.lock().unwrap() = Language::from_str(lang);
}

pub fn current() -> Language {
    *LANG.lock().unwrap()
}

/// Look up a translation key in the current language.
/// Returns the key itself if untranslated (graceful fallback).
pub fn tr(key: &str) -> &str {
    let lang = current();
    match lang {
        Language::Zh => zh(key),
        Language::En => en(key),
    }
}

// ── Macros for convenient access ──

/// Return the raw translated string (for use with `format!`).
#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::tr($key)
    };
}

/// Return a translated string, optionally formatting `{}` placeholders.
/// Uses simple `replacen()` under the hood (no compile-time format check,
/// but works with any type implementing `Display`).
#[macro_export]
macro_rules! tf {
    ($key:expr) => {
        $crate::i18n::tr($key).to_string()
    };
    ($key:expr, $($arg:expr),+ $(,)?) => {{
        let mut _r = $crate::i18n::tr($key).to_string();
        $(
            _r = _r.replacen("{}", &$arg.to_string(), 1);
        )+
        _r
    }};
}

// ── Chinese strings ──

fn zh(key: &str) -> &str {
    match key {
        // ── General ──
        "app.title" => "♪ 音源播放器",
        "app.ready" => "准备就绪 - 按 q 退出",
        "app.help_prompt" => "按 h/? 查看帮助 | ↑/↓ 选择 | Enter 播放",
        "app.quit" => "已退出音源播放器",
        "app.config_prompt" => "请先添加服务器 (按 Enter 编辑, Tab 切换字段, 保存后按 Esc)",
        "app.config_edit_hint" => "名称",
        "app.no_server" => "请先按 R 配置服务器地址",
        "app.unknown_artist" => "未知艺术家",
        "app.no_playing" => "当前没有正在播放的歌曲",
        "app.unknown_source" => "播放来源未知",

        // ── View modes ──
        "view.browse" => "音乐列表",
        "view.playlist_list" => "歌单管理",
        "view.playlist_content" => "歌单内容",
        "view.search" => "搜索",
        "view.search_results" => "搜索: {} ({} 结果)",
        "view.music_list" => "音乐列表 ({})",

        // ── Status messages ──
        "status.playing" => "▶ 正在播放: {}",
        "status.paused" => "已暂停",
        "status.stopped" => "已停止",
        "status.resumed" => "继续播放",
        "status.seek_forward" => "快进 → {}",
        "status.seek_backward" => "快退 ← {}",
        "status.volume" => "音量: {}%",
        "status.play_mode" => "播放模式: {}",
        "status.search_done" => "搜索完成: {} 个结果",
        "status.refreshing" => "正在刷新音乐列表...",
        "status.load_failed" => "加载失败",
        "status.loading" => "📂 正在加载: {}",
        "status.downloading" => "⏳ 正在下载: {}",
        "status.cache_reading" => "📖 从缓存读取: {}",
        "status.cache_read_fail" => "缓存读取失败: {}",
        "status.stream_fail" => "流式播放失败: {}",
        "status.decode_fail" => "解码失败 (可能是不支持的格式): {}",
        "status.download_fail" => "加载失败: {}",
        "status.single_repeat" => "单曲循环",
        "status.playlist_end" => "播放完毕",
        "status.shuffle_end" => "随机播放已播完",
        "status.servers_saved" => "已保存 {} 个服务器配置",
        "status.music_loaded" => "共 {} 首音乐，已加载 {} 首",
        "status.lang_switched" => "已切换为中文",
        "status.fetch_list_fail" => "获取音乐列表失败: {}",
        "status.seek_unavailable" => "文件还在下载中，请稍后再试",
        "status.jumped_to" => "已跳转到: {}",
        "status.jumped_to_playlist" => "已跳转到: {} — {}",
        "status.resuming" => "↻ 继续播放: {}",
        "status.sort_changed" => "排序: {}",

        // ── Play modes ──
        "playmode.sequential" => "顺序播放",
        "playmode.single_repeat" => "单曲循环",
        "playmode.shuffle" => "随机播放",
        "playmode.short_sequential" => "顺序",
        "playmode.short_single" => "单曲",
        "playmode.short_shuffle" => "随机",

        // ── Playlists ──
        "playlist.empty" => "还没有歌单 — 按 c 创建新歌单",
        "playlist.content_empty" => "歌单为空 — 在音乐列表中按 a 添加歌曲",
        "playlist.not_found" => "歌单不存在",
        "playlist.error" => "错误",
        "playlist.created" => "已创建歌单: {}",
        "playlist.default_name" => "歌单 {}",
        "playlist.added" => "已添加到歌单「{}」",
        "playlist.deleted" => "已删除歌单",
        "playlist.removed" => "已从歌单移除",
        "playlist.add_title" => "添加到歌单",
        "playlist.create_title" => "创建歌单",
        "playlist.create_hint" => "输入歌单名称:",
        "playlist.pick_hint" => "选择歌单 (Enter 确认)",
        "playlist.song_count" => "{} ({} 首)",

        // ── Play queue ──
        "queue.empty" => "队列为空 — 在列表中按 x 插播或 w 添加到队列",
        "queue.title" => "播放队列",
        "queue.current_label" => "当前播放: {}",
        "queue.count_single" => "1 首待播",
        "queue.count_multi" => "{} 首待播",
        "queue.added_front" => "已插播到队列首位 (队列共 {} 首)",
        "queue.added_back" => "已添加到队列末尾 (队列共 {} 首)",
        "queue.hint" => "↑/↓ 选择 | Enter 立即播放 | d 移除 | +/- 移动 | Esc 关闭",
        "queue.empty_hint" => "队列为空 — 在列表中按 x 插播或 w 添加到队列",

        // ── Config server ──
        "config.title_list" => "服务器管理 (Enter编辑 a添加 d删除 Space停用 Esc保存)",
        "config.title_edit" => "编辑服务器 (Tab切换 Enter保存 Esc取消)",
        "config.label_name" => "名称",
        "config.label_type" => "类型",
        "config.label_url" => "URL",
        "config.label_user" => "用户名",
        "config.label_password" => "密码",
        "config.enabled" => "已启用",
        "config.disabled" => "已停用",
        "config.empty" => "暂无服务器，按 a 添加",

        // ── Playback bar ──
        "playback.idle" => "空闲",
        "playback.volume" => "音量: {}%",

        // ── Search overlay ──
        "search.title" => "搜索",
        "search.prompt" => "输入搜索关键词:",

        // ── Help overlay ──
        "help.title" => "帮助",
        "help.nav_header" => "─ 导航 ─",
        "help.nav_up_down" => "↑/↓ 选择",
        "help.nav_page" => "PgUp / PgDn 翻页",
        "help.goto_playing" => "g 跳转到正在播放",
        "help.sort" => "S 切换排序方式",
        "help.search" => "/ 搜索",
        "help.playback_header" => "─ 播放控制 ─",
        "help.play" => "Enter 播放选中",
        "help.toggle" => "Space 暂停/继续",
        "help.stop" => "s 停止",
        "help.seek" => "←/→ 快退/快进 5秒",
        "help.play_mode" => "m 切换播放模式",
        "help.volume" => "+/- 音量增减",
        "help.queue_header" => "─ 播放队列 ─",
        "help.queue_play_next" => "x 插播到队列首位",
        "help.queue_add" => "w 添加到队列末尾",
        "help.queue_view" => "u 查看/管理播放队列",
        "help.playlist_header" => "─ 歌单操作 ─",
        "help.playlist_add" => "a 加入歌单",
        "help.playlist_manage" => "l 歌单管理 / 返回",
        "help.playlist_create" => "c 创建歌单",
        "help.playlist_delete" => "d 删除歌单 / 移出",
        "help.system_header" => "─ 系统 ─",
        "help.refresh" => "r 刷新音乐列表",
        "help.config" => "R 配置服务器地址",
        "help.language" => "L 切换语言",
        "help.help" => "h / ? 本帮助",
        "help.quit" => "q / Esc 退出 / 返回",

        // ── Sort ──
        "sort.default" => "默认排序",
        "sort.name" => "按歌名",
        "sort.artist" => "按艺术家",
        "sort.duration" => "按时长",

        // ── Config server list phase ──
        "config.edit_hint_name" => "名称",
        "config.edit_hint_url" => "URL",
        "config.edit_hint_user" => "用户名",
        "config.edit_hint_password" => "密码",

        // ── Misc ──
        "misc.quick_filter" => "按服务器",
        "misc.servers_count" => "{} 个服务器",
        "header.title" => " ♪ 音源播放器 [{}]{}  {}  |  {} 首  {} 歌单",

        _ => key,
    }
}

// ── English strings ──

fn en(key: &str) -> &str {
    match key {
        // ── General ──
        "app.title" => "♪ Tune Player",
        "app.ready" => "Ready — press q to quit",
        "app.help_prompt" => "Press h/? for help | ↑/↓ select | Enter play",
        "app.quit" => "Goodbye!",
        "app.config_prompt" => "Add a server (Enter edit, Tab switch field, Esc save & exit)",
        "app.config_edit_hint" => "Name",
        "app.no_server" => "Press R to configure server URL",
        "app.unknown_artist" => "Unknown Artist",
        "app.no_playing" => "No track is currently playing",
        "app.unknown_source" => "Unknown source",

        // ── View modes ──
        "view.browse" => "Music List",
        "view.playlist_list" => "Playlists",
        "view.playlist_content" => "Playlist",
        "view.search" => "Search",
        "view.search_results" => "Search: {} ({} results)",
        "view.music_list" => "Music List ({})",

        // ── Status messages ──
        "status.playing" => "▶ Now playing: {}",
        "status.paused" => "Paused",
        "status.stopped" => "Stopped",
        "status.resumed" => "Resumed",
        "status.seek_forward" => "Seek → {}",
        "status.seek_backward" => "Seek ← {}",
        "status.volume" => "Volume: {}%",
        "status.play_mode" => "Mode: {}",
        "status.search_done" => "Search finished: {} results",
        "status.refreshing" => "Refreshing music list...",
        "status.load_failed" => "Load failed",
        "status.loading" => "📂 Loading: {}",
        "status.downloading" => "⏳ Downloading: {}",
        "status.cache_reading" => "📖 Reading from cache: {}",
        "status.cache_read_fail" => "Cache read error: {}",
        "status.stream_fail" => "Streaming failed: {}",
        "status.decode_fail" => "Decode failed (unsupported format?): {}",
        "status.download_fail" => "Load failed: {}",
        "status.single_repeat" => "Single-repeat",
        "status.playlist_end" => "End of playlist",
        "status.shuffle_end" => "Shuffle finished",
        "status.servers_saved" => "Saved {} server(s)",
        "status.music_loaded" => "{} songs loaded",
        "status.lang_switched" => "Switched to English",
        "status.fetch_list_fail" => "Failed to fetch music list: {}",
        "status.seek_unavailable" => "Download in progress, please wait before seeking",
        "status.jumped_to" => "Jumped to: {}",
        "status.jumped_to_playlist" => "Jumped to: {} — {}",
        "status.resuming" => "↻ Resuming: {}",
        "status.sort_changed" => "Sort: {}",

        // ── Play modes ──
        "playmode.sequential" => "Sequential",
        "playmode.single_repeat" => "Single Repeat",
        "playmode.shuffle" => "Shuffle",
        "playmode.short_sequential" => "Seq",
        "playmode.short_single" => "Repeat",
        "playmode.short_shuffle" => "Shuffle",

        // ── Playlists ──
        "playlist.empty" => "No playlists — press c to create one",
        "playlist.content_empty" => "Playlist is empty — press a in music list to add songs",
        "playlist.not_found" => "Playlist not found",
        "playlist.error" => "Error",
        "playlist.created" => "Created playlist: {}",
        "playlist.default_name" => "Playlist {}",
        "playlist.added" => "Added to playlist \"{}\"",
        "playlist.deleted" => "Playlist deleted",
        "playlist.removed" => "Removed from playlist",
        "playlist.add_title" => "Add to Playlist",
        "playlist.create_title" => "Create Playlist",
        "playlist.create_hint" => "Enter playlist name:",
        "playlist.pick_hint" => "Choose a playlist (Enter to confirm)",
        "playlist.song_count" => "{} ({} songs)",

        // ── Play queue ──
        "queue.empty" => "Queue is empty — press x to play next or w to add to queue",
        "queue.title" => "Play Queue",
        "queue.current_label" => "Now playing: {}",
        "queue.count_single" => "1 song queued",
        "queue.count_multi" => "{} songs queued",
        "queue.added_front" => "Queued as next ({} total)",
        "queue.added_back" => "Added to end of queue ({} total)",
        "queue.hint" => "↑/↓ select | Enter play | d remove | +/- move | Esc close",
        "queue.empty_hint" => "Queue is empty — press x to play next or w to queue",

        // ── Config server ──
        "config.title_list" => "Server Manager (Enter edit a add d delete Space toggle Esc save)",
        "config.title_edit" => "Edit Server (Tab switch Enter save Esc cancel)",
        "config.label_name" => "Name",
        "config.label_type" => "Type",
        "config.label_url" => "URL",
        "config.label_user" => "Username",
        "config.label_password" => "Password",
        "config.enabled" => "Enabled",
        "config.disabled" => "Disabled",
        "config.empty" => "No servers, press a to add",

        // ── Playback bar ──
        "playback.idle" => "Idle",
        "playback.volume" => "Vol: {}%",

        // ── Search overlay ──
        "search.title" => "Search",
        "search.prompt" => "Type to search:",

        // ── Help overlay ──
        "help.title" => "Help",
        "help.nav_header" => "─ Navigation ─",
        "help.nav_up_down" => "↑/↓ Select",
        "help.nav_page" => "PgUp / PgDn Scroll page",
        "help.goto_playing" => "g Jump to now playing",
        "help.sort" => "S Cycle sort mode",
        "help.search" => "/ Search",
        "help.playback_header" => "─ Playback ─",
        "help.play" => "Enter Play selected",
        "help.toggle" => "Space Pause / Resume",
        "help.stop" => "s Stop",
        "help.seek" => "←/→ Seek 5s",
        "help.play_mode" => "m Cycle play mode",
        "help.volume" => "+/- Volume",
        "help.queue_header" => "─ Play Queue ─",
        "help.queue_play_next" => "x Queue as next",
        "help.queue_add" => "w Add to queue end",
        "help.queue_view" => "u View / manage queue",
        "help.playlist_header" => "─ Playlists ─",
        "help.playlist_add" => "a Add to playlist",
        "help.playlist_manage" => "l Playlist mgmt / back",
        "help.playlist_create" => "c Create playlist",
        "help.playlist_delete" => "d Delete / remove",
        "help.system_header" => "─ System ─",
        "help.refresh" => "r Refresh music list",
        "help.config" => "R Configure servers",
        "help.language" => "L Switch language",
        "help.help" => "h / ? Help",
        "help.quit" => "q / Esc Quit / back",

        // ── Sort ──
        "sort.default" => "Default",
        "sort.name" => "By name",
        "sort.artist" => "By artist",
        "sort.duration" => "By duration",

        // ── Config server list phase ──
        "config.edit_hint_name" => "Name",
        "config.edit_hint_url" => "URL",
        "config.edit_hint_user" => "Username",
        "config.edit_hint_password" => "Password",

        // ── Misc ──
        "misc.quick_filter" => "by server",
        "misc.servers_count" => "{} servers",
        "header.title" => " ♪ Tune Player [{}]{}  {}  |  {} songs  {} playlists",

        _ => key,
    }
}
