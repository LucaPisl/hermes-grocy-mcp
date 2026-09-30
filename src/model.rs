use crate::{
    client::ApiBase,
    error::{AppError, Result},
};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccessMode {
    ReadOnly,
    Full,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransportPolicy {
    HttpsOnly,
    LoopbackDevelopment,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderIdentity {
    pub bus_name: String,
    pub executable: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileSelection {
    pub profile: String,
    pub provider: ProviderIdentity,
}
pub struct PrivateProfile {
    pub api_base: ApiBase,
    pub api_key: SecretString,
    pub access: AccessMode,
    pub timezone: Option<String>,
    pub defaults: Value,
    pub ca_path: Option<PathBuf>,
    pub transport: TransportPolicy,
    pub unsafe_storage: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Page {
    pub offset: usize,
    pub limit: usize,
}
impl Default for Page {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 100,
        }
    }
}
impl Page {
    pub fn new(offset: usize, limit: usize) -> Result<Self> {
        if !(1..=500).contains(&limit) || offset.checked_add(limit + 1).is_none() {
            return Err(AppError::input());
        }
        Ok(Self { offset, limit })
    }
    pub fn slice(&self, items: Vec<Value>) -> Value {
        let total = items.len();
        let data = items
            .into_iter()
            .skip(self.offset)
            .take(self.limit)
            .collect::<Vec<_>>();
        let more = self.offset.saturating_add(self.limit) < total;
        serde_json::json!({"items":data,"total":total,"has_more":more,"next_offset":if more{Some(self.offset+self.limit)}else{None}})
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordId(pub u64);
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WriteStatus {
    Completed,
}
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    Confirmed,
    Incomplete,
    Unavailable,
}
#[derive(Serialize)]
pub struct WriteReport {
    pub status: WriteStatus,
    pub verification: Verification,
    pub target: Value,
    pub receipt: Value,
}
pub struct Attachment {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub name: String,
}
pub struct ToolOutput {
    pub data: Value,
    pub write: Option<WriteReport>,
    pub attachment: Option<Attachment>,
}
impl ToolOutput {
    pub fn data(data: Value) -> Self {
        Self {
            data,
            write: None,
            attachment: None,
        }
    }
    pub fn json(&self) -> Value {
        serde_json::json!({"data":self.data,"write":self.write})
    }
}
