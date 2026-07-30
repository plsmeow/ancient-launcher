use serde::{Deserialize, Serialize};
use crate::HTTP_CLIENT;

const GITHUB_API: &str = "https://api.github.com/repos/plsmeow/ancient/releases/latest";
const JAR_NAME: &str = "onetap-1.0.0.jar";

#[derive(Serialize, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    pub name: String,
    pub body: Option<String>,
    pub assets: Vec<GithubAsset>,
}

#[derive(Serialize, Deserialize)]
pub struct GithubAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: Option<i64>,
}

pub async fn fetch_latest_release() -> Result<GithubRelease, String> {
    HTTP_CLIENT
        .get(GITHUB_API)
        .header("User-Agent", "ancient-launcher/1.0.0")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch latest release: {}", e))?
        .json::<GithubRelease>()
        .await
        .map_err(|e| format!("Failed to parse release: {}", e))
}

pub fn get_jar_url(release: &GithubRelease) -> Option<&str> {
    release.assets.iter()
        .find(|a| a.name == JAR_NAME)
        .map(|a| a.browser_download_url.as_str())
}
