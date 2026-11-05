//! `pr-begger request`: post or update the CR-kind comment on a PR.

use anyhow::{bail, Result};

use crate::github::Client;
use crate::kind::Kind;
use crate::marker::{self, Marker, Plan};
use crate::target::PrRef;

pub struct Outcome {
    pub url: String,
    pub updated: bool,
}

/// Post the request, or edit the viewer's existing request comment so a PR
/// never collects a pile of them.
pub fn request(client: &Client, pr: &PrRef, kind: Kind, why: Option<&str>) -> Result<Outcome> {
    let info = client.pr_comments(pr)?;
    if info.state != "OPEN" {
        bail!("{} is {}, not open", info.url, info.state.to_lowercase());
    }
    let body = Marker::new(kind, why).to_comment_body();
    match marker::plan_request(&info.comments, &info.viewer) {
        Plan::Update { comment_id } => {
            let url = client.update_comment(&comment_id, &body)?;
            Ok(Outcome { url, updated: true })
        }
        Plan::Create => {
            let url = client.add_comment(&info.id, &body)?;
            Ok(Outcome {
                url,
                updated: false,
            })
        }
    }
}
