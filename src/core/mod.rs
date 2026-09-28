pub mod commands;
pub mod docker;

use anyhow::Result;
use async_trait::async_trait;
use futures_util::stream::BoxStream;
use std::pin::Pin;
use tokio::io::AsyncWrite;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Containers,
    Images,
    Volumes,
    Networks,
    Compose,
}
impl Kind {
    pub const ALL: [Self; 5] = [
        Self::Containers,
        Self::Images,
        Self::Volumes,
        Self::Networks,
        Self::Compose,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Self::Containers => "Containers",
            Self::Images => "Images",
            Self::Volumes => "Volumes",
            Self::Networks => "Networks",
            Self::Compose => "Compose",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Resource {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub state: String,
    pub extra: String,
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub engine: String,
    pub compose_members: std::collections::HashMap<String, String>,
    pub containers: Vec<Resource>,
    pub images: Vec<Resource>,
    pub volumes: Vec<Resource>,
    pub networks: Vec<Resource>,
    pub compose: Vec<Resource>,
}
impl Snapshot {
    pub fn resources(&self, kind: Kind) -> &[Resource] {
        match kind {
            Kind::Containers => &self.containers,
            Kind::Images => &self.images,
            Kind::Volumes => &self.volumes,
            Kind::Networks => &self.networks,
            Kind::Compose => &self.compose,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Action {
    Start,
    Stop,
    Restart,
    Pause,
    Resume,
    Delete,
}
#[derive(Clone, Debug, Default)]
pub struct Metrics {
    pub cpu: f64,
    pub memory: u64,
    pub limit: u64,
    pub rx: u64,
    pub tx: u64,
}
pub struct ShellSession {
    pub id: String,
    pub output: BoxStream<'static, Result<Vec<u8>>>,
    pub input: Pin<Box<dyn AsyncWrite + Send>>,
}
#[derive(Clone, Debug, Default)]
pub struct RunSpec {
    pub image: String,
    pub name: String,
    pub command: String,
    pub ports: String,
    pub env: String,
    pub mounts: String,
    pub network: String,
    pub interactive: bool,
}
// The UI consumes these models and this contract, never Bollard types.
#[async_trait]
pub trait Engine: Send + Sync {
    fn endpoint(&self) -> &str;
    fn docker_cli(&self) -> std::process::Command {
        commands::docker_cli(&crate::config::Endpoint {
            host: self.endpoint().into(),
            certificates: None,
        })
    }
    async fn snapshot(&self) -> Result<Snapshot>;
    async fn action(&self, id: &str, action: Action) -> Result<()>;
    async fn logs(&self, id: &str) -> Result<BoxStream<'static, Result<String>>>;
    async fn stats(&self, id: &str) -> Result<BoxStream<'static, Result<Metrics>>>;
    async fn inspect(&self, kind: Kind, id: &str) -> Result<String>;
    async fn shell(&self, id: &str, command: &str, cols: u16, rows: u16) -> Result<ShellSession>;
    async fn compose_up(&self, path: &str) -> Result<String>;
    async fn create(&self, spec: RunSpec) -> Result<String>;
    async fn web_urls(&self, id: &str) -> Result<Vec<String>>;
    async fn docker_command(&self, command: &str) -> Result<String>;
    async fn resize_shell(&self, id: &str, cols: u16, rows: u16) -> Result<()>;
}
