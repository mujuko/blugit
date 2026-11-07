use crate::model::{Peer, ShareId};
use serde::{Deserialize, Serialize};

pub(super) const MAGIC: &[u8] = b"BLUGIT/2\n";
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Operation {
    Enroll,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub operation: Operation,
    pub share: Option<ShareId>,
    pub peer: Peer,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Reply {
    pub share: ShareId,
    pub host: Peer,
    pub nonce: [u8; 16],
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "status", deny_unknown_fields)]
pub(super) enum Response {
    Ready { reply: Reply },
    Rejected { reason: String },
}
pub(super) fn transcript(request: &Request, reply: &Reply) -> anyhow::Result<Vec<u8>> {
    Ok(serde_json::to_vec(&(
        "blugit/v2/enrollment",
        request,
        reply,
    ))?)
}
