/// 文件闪传 (Fast File Transfer) adapter.
///
/// Endpoints:
/// - `GET {base}/musicsV2` → `{ children: [...] }`
/// - `GET {base}/file?path={encoded_path}` → audio stream

use anyhow::Result;
use async_trait::async_trait;

use crate::server::{encode_url_component, MusicEntry, MusicListResponse, MusicServer, HTTP};

/// Adapter for the 文件闪传 mobile app server.
pub struct FileTransferServer {
    base_url: String,
}

impl FileTransferServer {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
        }
    }
}

#[async_trait]
impl MusicServer for FileTransferServer {
    fn name(&self) -> &str {
        "文件闪传"
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn fetch_list(&self) -> Result<Vec<MusicEntry>> {
        let url = format!("{}/musicsV2", self.base_url);
        let resp: MusicListResponse = HTTP.get(&url).send().await?.json().await?;
        Ok(resp.children)
    }

    fn stream_url(&self, music: &MusicEntry) -> String {
        let encoded_path = encode_url_path(&music.absolute_path);
        format!("{}/file?path={}", self.base_url, encoded_path)
    }
}

fn encode_url_path(path: &str) -> String {
    path.split('/')
        .map(encode_url_component)
        .collect::<Vec<_>>()
        .join("/")
}
