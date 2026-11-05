//! GitHub GraphQL access.
//!
//! One search per configured org/repo, run in parallel threads: GitHub runs
//! aliased searches inside one request serially, so separate requests finish
//! in the time of the slowest rather than the sum. Every field a row draws
//! comes back in that single search, including the comments the CR-kind
//! marker is read from, so there is no per-PR follow-up request.

use std::collections::HashSet;
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::Config;
use crate::marker::{self, Comment, SEARCH_TOKEN};
use crate::model::{Activity, Ci, Decision, Merge, Pr, Request, Review};
use crate::target::PrRef;

const ENDPOINT: &str = "https://api.github.com/graphql";
const PAGE: usize = 100;
/// Smallest batch of PRs per detail lookup, and how many lookups may run at
/// once (bigger scopes get bigger batches rather than more connections).
const DETAIL_BATCH: usize = 5;
const MAX_PARALLEL: usize = 8;

/// GitHub gave up on a query (502/504), usually because it was too heavy.
#[derive(Debug)]
struct TimedOut;

impl std::fmt::Display for TimedOut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GitHub timed out answering the query (502); try again or narrow the scope")
    }
}

impl std::error::Error for TimedOut {}

pub struct Client {
    agent: ureq::Agent,
    token: String,
}

/// Token from `GITHUB_TOKEN` / `GH_TOKEN`, else from the `gh` login.
pub fn token() -> Result<String> {
    for var in ["GITHUB_TOKEN", "GH_TOKEN"] {
        if let Ok(t) = std::env::var(var) {
            if !t.trim().is_empty() {
                return Ok(t.trim().to_string());
            }
        }
    }
    let out = Command::new("gh")
        .args(["auth", "token"])
        .output()
        .context("no GITHUB_TOKEN set and could not run `gh auth token` (is gh installed?)")?;
    let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || t.is_empty() {
        bail!("no GitHub token: run `gh auth login` or set GITHUB_TOKEN");
    }
    Ok(t)
}

#[derive(Deserialize)]
struct GqlResponse<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GqlError>,
}

#[derive(Deserialize)]
struct GqlError {
    message: String,
}

impl Client {
    pub fn new(token: String) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .user_agent("pr-begger")
            .build()
            .into();
        Client { agent, token }
    }

    pub fn from_env() -> Result<Self> {
        Ok(Self::new(token()?))
    }

    fn graphql<T: DeserializeOwned>(&self, query: &str, variables: Value) -> Result<T> {
        let started = std::time::Instant::now();
        let body = json!({ "query": query, "variables": variables }).to_string();
        let mut resp = self
            .agent
            .post(ENDPOINT)
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .send(body.as_str())
            .context("talking to api.github.com")?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_string()
            .context("reading GitHub response")?;
        if std::env::var_os("PR_BEGGER_DEBUG").is_some() {
            let what = query
                .split_whitespace()
                .take(3)
                .collect::<Vec<_>>()
                .join(" ");
            eprintln!(
                "[pr-begger] {status} {:>5}ms {:>8}B  {what}",
                started.elapsed().as_millis(),
                text.len()
            );
        }
        match status {
            200 => {}
            401 => {
                bail!("GitHub rejected the token (401): run `gh auth login` or set GITHUB_TOKEN")
            }
            403 | 429 => bail!(
                "GitHub refused the request ({status}), probably rate limited: {}",
                snippet(&text)
            ),
            502 | 504 => return Err(TimedOut.into()),
            _ => bail!("GitHub returned {status}: {}", snippet(&text)),
        }
        decode(&text)
    }

    /// Open PRs carrying a CR-kind marker, across every configured scope.
    pub fn fetch_board(&self, cfg: &Config) -> Result<Vec<Pr>> {
        let prs = self.fetch(cfg, false, |scope| {
            format!("is:pr is:open archived:false \"{SEARCH_TOKEN}\" in:comments {scope}")
        })?;
        // The search matched the word; only a readable marker makes it a request.
        Ok(prs.into_iter().filter(|p| p.request.is_some()).collect())
    }

    /// The viewer's own open PRs, with conversation activity.
    pub fn fetch_mine(&self, cfg: &Config) -> Result<Vec<Pr>> {
        self.fetch(cfg, true, |scope| {
            format!("is:pr is:open archived:false author:@me {scope}")
        })
    }

    /// Two phases. The search returns only PR ids, which is fast. Everything
    /// a row shows is then looked up by id in small batches run in parallel:
    /// GitHub resolves nested fields (comments, threads, CI rollup,
    /// mergeability) one PR at a time within a request, so one 40-PR search
    /// asking for all of it took ~7s where eight parallel batches take ~2s.
    fn fetch(
        &self,
        cfg: &Config,
        mine: bool,
        query: impl Fn(&str) -> String + Sync,
    ) -> Result<Vec<Pr>> {
        let ids = self.search_scopes(cfg, query)?;
        self.details(&ids, mine)
    }

    fn details(&self, ids: &[String], mine: bool) -> Result<Vec<Pr>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let chunk = ids.len().div_ceil(MAX_PARALLEL).max(DETAIL_BATCH);
        let results: Vec<Result<Vec<Pr>>> = std::thread::scope(|s| {
            let handles: Vec<_> = ids
                .chunks(chunk)
                .map(|ids| s.spawn(move || self.detail_batch(ids, mine)))
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join()
                        .unwrap_or_else(|_| Err(anyhow!("detail thread panicked")))
                })
                .collect()
        });
        let mut out = Vec::with_capacity(ids.len());
        for r in results {
            out.extend(r.context("fetching PR details")?);
        }
        Ok(out)
    }

    fn detail_batch(&self, ids: &[String], mine: bool) -> Result<Vec<Pr>> {
        let res: Result<NodesData> =
            self.graphql(DETAIL_QUERY, json!({ "ids": ids, "mine": mine }));
        match res {
            // A batch of PRs with huge threads can still be too slow for
            // GitHub; split it rather than fail the whole board.
            Err(e) if e.is::<TimedOut>() && ids.len() > 1 => {
                let (a, b) = ids.split_at(ids.len() / 2);
                let mut out = self.detail_batch(a, mine)?;
                out.extend(self.detail_batch(b, mine)?);
                Ok(out)
            }
            r => Ok(r?
                .nodes
                .into_iter()
                .flatten()
                .filter_map(PrNode::into_pr)
                .collect()),
        }
    }

    /// PR ids matching the query in any scope, deduplicated, in search order.
    fn search_scopes(
        &self,
        cfg: &Config,
        query: impl Fn(&str) -> String + Sync,
    ) -> Result<Vec<String>> {
        let scopes = cfg.scopes();
        let results: Vec<Result<Vec<String>>> = std::thread::scope(|s| {
            let handles: Vec<_> = scopes
                .iter()
                .map(|scope| {
                    let q = query(scope);
                    s.spawn(move || self.search(&q, cfg.max_prs))
                })
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join()
                        .unwrap_or_else(|_| Err(anyhow!("search thread panicked")))
                })
                .collect()
        });
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for (scope, r) in scopes.iter().zip(results) {
            for id in r.with_context(|| format!("searching {scope}"))? {
                if seen.insert(id.clone()) {
                    out.push(id);
                }
            }
        }
        Ok(out)
    }

    fn search(&self, q: &str, max: usize) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let n = PAGE.min(max - out.len());
            let data: SearchData =
                self.graphql(SEARCH_QUERY, json!({ "q": q, "n": n, "after": after }))?;
            let page = data.search;
            out.extend(page.nodes.into_iter().flatten().filter_map(|n| n.id));
            if !page.page_info.has_next_page || out.len() >= max {
                break;
            }
            after = page.page_info.end_cursor;
            if after.is_none() {
                break;
            }
        }
        Ok(out)
    }

    /// Everything `request` needs about one PR.
    pub fn pr_comments(&self, pr: &PrRef) -> Result<PrComments> {
        let data: PrCommentsData = self.graphql(
            PR_COMMENTS_QUERY,
            json!({ "o": pr.owner, "r": pr.repo, "n": pr.number }),
        )?;
        let node = data
            .repository
            .and_then(|r| r.pull_request)
            .ok_or_else(|| anyhow!("{pr} not found (or no access)"))?;
        Ok(PrComments {
            viewer: data.viewer.login,
            id: node.id,
            url: node.url,
            state: node.state,
            comments: node.comments.nodes.into_iter().map(Into::into).collect(),
        })
    }

    pub fn add_comment(&self, subject_id: &str, body: &str) -> Result<String> {
        let v: Value = self.graphql(
            "mutation($s:ID!,$b:String!){addComment(input:{subjectId:$s,body:$b}){commentEdge{node{url}}}}",
            json!({ "s": subject_id, "b": body }),
        )?;
        Ok(v.pointer("/addComment/commentEdge/node/url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    pub fn update_comment(&self, comment_id: &str, body: &str) -> Result<String> {
        let v: Value = self.graphql(
            "mutation($i:ID!,$b:String!){updateIssueComment(input:{id:$i,body:$b}){issueComment{url}}}",
            json!({ "i": comment_id, "b": body }),
        )?;
        Ok(v.pointer("/updateIssueComment/issueComment/url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }
}

fn snippet(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() > 200 {
        format!("{}…", s.chars().take(200).collect::<String>())
    } else {
        s.to_string()
    }
}

/// Decode a GraphQL body. Errors are fatal only when no data came back: a
/// partial answer (one repo behind SSO, say) still shows what it can.
fn decode<T: DeserializeOwned>(text: &str) -> Result<T> {
    let resp: GqlResponse<T> = serde_json::from_str(text).context("decoding GitHub response")?;
    match resp.data {
        Some(d) => Ok(d),
        None => {
            let msgs: Vec<_> = resp.errors.into_iter().map(|e| e.message).collect();
            if msgs.is_empty() {
                bail!("GitHub returned no data");
            }
            bail!("GitHub: {}", msgs.join("; "))
        }
    }
}

pub struct PrComments {
    pub viewer: String,
    pub id: String,
    pub url: String,
    pub state: String,
    pub comments: Vec<Comment>,
}

const SEARCH_QUERY: &str = r#"
query($q: String!, $n: Int!, $after: String) {
  search(query: $q, type: ISSUE, first: $n, after: $after) {
    pageInfo { hasNextPage endCursor }
    nodes { ... on PullRequest { id } }
  }
}"#;

const DETAIL_QUERY: &str = r#"
query($ids: [ID!]!, $mine: Boolean!) {
  nodes(ids: $ids) {
    ... on PullRequest {
      id number title url isDraft createdAt updatedAt additions deletions
      author { login }
      repository { nameWithOwner }
      reviewDecision mergeable mergeStateStatus
      reviewRequests { totalCount }
      latestOpinionatedReviews(first: 20) {
        nodes { state submittedAt author { __typename login } }
      }
      commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
      comments(last: 40) {
        totalCount
        nodes { id body createdAt lastEditedAt author { __typename login } }
      }
      reviewThreads(first: 100) @include(if: $mine) {
        totalCount
        nodes {
          isResolved
          comments(last: 1) { nodes { createdAt author { __typename login } } }
        }
      }
    }
  }
}"#;

#[derive(Deserialize)]
struct NodesData {
    #[serde(default)]
    nodes: Vec<Option<PrNode>>,
}

fn decision(s: Option<&str>) -> Decision {
    match s {
        Some("APPROVED") => Decision::Approved,
        Some("CHANGES_REQUESTED") => Decision::ChangesRequested,
        Some("REVIEW_REQUIRED") => Decision::Required,
        _ => Decision::None,
    }
}

fn rollup(commits: Option<&Nodes<CommitNode>>) -> Ci {
    Ci::from_rollup(
        commits
            .and_then(|c| c.nodes.last())
            .and_then(|c| c.commit.status_check_rollup.as_ref())
            .map(|r| r.state.as_str()),
    )
}

const PR_COMMENTS_QUERY: &str = r#"
query($o: String!, $r: String!, $n: Int!) {
  viewer { login }
  repository(owner: $o, name: $r) {
    pullRequest(number: $n) {
      id url state
      comments(last: 100) {
        nodes { id body createdAt lastEditedAt author { __typename login } }
      }
    }
  }
}"#;

#[derive(Deserialize)]
struct SearchData {
    search: SearchPage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchPage {
    page_info: PageInfo,
    #[serde(default)]
    nodes: Vec<Option<PrNode>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Actor {
    login: String,
    #[serde(rename = "__typename", default)]
    typename: String,
}

impl Actor {
    fn is_bot(&self) -> bool {
        self.typename == "Bot" || self.login.ends_with("[bot]")
    }
}

#[derive(Deserialize)]
struct Count {
    #[serde(rename = "totalCount")]
    total_count: u32,
}

#[derive(Deserialize)]
struct Nodes<T> {
    #[serde(default = "Vec::new")]
    nodes: Vec<T>,
    #[serde(rename = "totalCount", default)]
    total_count: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentNode {
    id: String,
    body: String,
    created_at: DateTime<Utc>,
    last_edited_at: Option<DateTime<Utc>>,
    author: Option<Actor>,
}

impl From<CommentNode> for Comment {
    fn from(c: CommentNode) -> Self {
        Comment {
            id: c.id,
            author: c.author.map(|a| a.login),
            body: c.body,
            created_at: c.created_at,
            last_edited_at: c.last_edited_at,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewNode {
    state: String,
    submitted_at: Option<DateTime<Utc>>,
    author: Option<Actor>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadNode {
    is_resolved: bool,
    comments: Nodes<ThreadComment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadComment {
    created_at: DateTime<Utc>,
    author: Option<Actor>,
}

#[derive(Deserialize)]
struct CommitNode {
    commit: Commit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Commit {
    status_check_rollup: Option<Rollup>,
}

#[derive(Deserialize)]
struct Rollup {
    state: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repo {
    name_with_owner: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrNode {
    id: Option<String>,
    number: Option<u64>,
    title: Option<String>,
    url: Option<String>,
    #[serde(default)]
    is_draft: bool,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
    author: Option<Actor>,
    repository: Option<Repo>,
    review_decision: Option<String>,
    mergeable: Option<String>,
    merge_state_status: Option<String>,
    review_requests: Option<Count>,
    latest_opinionated_reviews: Option<Nodes<ReviewNode>>,
    commits: Option<Nodes<CommitNode>>,
    comments: Option<Nodes<CommentNode>>,
    review_threads: Option<Nodes<ThreadNode>>,
}

impl PrNode {
    /// `None` for search hits that are not PRs (the inline fragment leaves
    /// them as empty objects).
    fn into_pr(self) -> Option<Pr> {
        let author = self
            .author
            .as_ref()
            .map(|a| a.login.clone())
            .unwrap_or_else(|| "ghost".into());
        let reviews = self
            .latest_opinionated_reviews
            .map(|r| r.nodes)
            .unwrap_or_default();
        let approvals = reviews.iter().filter(|r| r.state == "APPROVED").count() as u32;
        let decision = decision(self.review_decision.as_deref());
        let ci = rollup(self.commits.as_ref());
        let other =
            |a: &Option<Actor>| a.as_ref().is_some_and(|a| !a.is_bot() && a.login != author);
        let comments_total = self.comments.as_ref().map_or(0, |c| c.total_count);
        let comment_nodes = self.comments.map(|c| c.nodes).unwrap_or_default();
        // Human comments by others, minus CR-kind requests (those are not news).
        let other_comment_times: Vec<DateTime<Utc>> = comment_nodes
            .iter()
            .filter(|c| other(&c.author) && marker::parse(&c.body).is_none())
            .map(|c| c.created_at)
            .collect();
        let comments: Vec<Comment> = comment_nodes.into_iter().map(Comment::from).collect();
        let request = marker::newest(&comments).map(|f| Request {
            kind: f.marker.kind,
            why: f.marker.why,
            by: f.comment.author.clone(),
            at: f.comment.effective_at(),
        });

        let activity = self.review_threads.map(|threads| {
            let mut last: Option<DateTime<Utc>> = None;
            let mut bump = |t: DateTime<Utc>| last = Some(last.map_or(t, |l| l.max(t)));
            let mut unresolved = 0;
            for t in &threads.nodes {
                if !t.is_resolved {
                    unresolved += 1;
                }
                for c in t.comments.nodes.iter().filter(|c| other(&c.author)) {
                    bump(c.created_at);
                }
            }
            for r in reviews.iter().filter(|r| other(&r.author)) {
                if let Some(t) = r.submitted_at {
                    bump(t);
                }
            }
            for &t in &other_comment_times {
                bump(t);
            }
            Activity {
                unresolved_threads: unresolved,
                total_threads: threads.total_count.max(threads.nodes.len() as u32),
                comments: comments_total,
                last_by_others: last,
            }
        });

        Some(Pr {
            id: self.id?,
            repo: self.repository?.name_with_owner,
            number: self.number?,
            title: self.title.unwrap_or_default(),
            url: self.url?,
            author,
            created_at: self.created_at?,
            updated_at: self.updated_at?,
            additions: self.additions,
            deletions: self.deletions,
            draft: self.is_draft,
            ci,
            review: Review {
                decision,
                approvals,
                pending: self.review_requests.map_or(0, |r| r.total_count),
            },
            merge: Merge::from_github(
                self.mergeable.as_deref(),
                self.merge_state_status.as_deref(),
                self.is_draft,
            ),
            request,
            activity,
        })
    }
}

#[derive(Deserialize)]
struct Viewer {
    login: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrCommentsData {
    viewer: Viewer,
    repository: Option<RepoPr>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepoPr {
    pull_request: Option<PrCommentsNode>,
}

#[derive(Deserialize)]
struct PrCommentsNode {
    id: String,
    url: String,
    state: String,
    comments: Nodes<CommentNode>,
}

/// Parse a raw search response. Exposed for tests with recorded payloads.
#[cfg(test)]
pub fn parse_search_response(text: &str) -> Result<Vec<Pr>> {
    let data: SearchData = decode(text)?;
    Ok(data
        .search
        .nodes
        .into_iter()
        .flatten()
        .filter_map(PrNode::into_pr)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kind::Kind;

    const FIXTURE: &str = include_str!("../tests/fixtures/search.json");

    #[test]
    fn parses_recorded_search() {
        let prs = parse_search_response(FIXTURE).unwrap();
        assert_eq!(prs.len(), 3, "the issue-shaped node is skipped");

        let a = &prs[0];
        assert_eq!(a.key(), "Autofleet/api#101");
        assert_eq!(a.kind(), Some(Kind::Design), "the newer marker wins");
        let req = a.request.as_ref().unwrap();
        assert_eq!(req.by.as_deref(), Some("carol"));
        assert_eq!(req.why.as_deref(), Some("new queue topology"));
        assert_eq!(a.ci, Ci::Failing);
        assert_eq!(a.review.decision, Decision::Required);
        assert_eq!(a.review.pending, 2);
        assert_eq!(a.merge, Merge::Behind);
        assert_eq!((a.additions, a.deletions), (120, 30));
        assert!(a.activity.is_none(), "board searches skip threads");

        let b = &prs[1];
        assert_eq!(b.kind(), None, "matched the search word but has no marker");
        assert_eq!(b.merge, Merge::Draft);
        assert_eq!(b.ci, Ci::None);

        let c = &prs[2];
        assert_eq!(c.kind(), Some(Kind::RubberStamp));
        assert_eq!(c.review.decision, Decision::Approved);
        assert_eq!(c.review.approvals, 1);
        let act = c.activity.as_ref().unwrap();
        assert_eq!(act.unresolved_threads, 1);
        assert_eq!(act.total_threads, 2);
        assert_eq!(
            act.last_by_others.unwrap().to_rfc3339(),
            "2026-11-04T12:00:00+00:00",
            "bots and the author do not count as activity"
        );
    }

    #[test]
    fn detail_lookup_parses_nodes_and_skips_nulls() {
        let text = r#"{"data":{"nodes":[null,{"id":"PR_x","number":3,"title":"t",
            "url":"https://github.com/o/r/pull/3","isDraft":false,
            "createdAt":"2026-11-01T00:00:00Z","updatedAt":"2026-11-01T00:00:00Z",
            "additions":1,"deletions":2,"author":{"login":"a"},
            "repository":{"nameWithOwner":"o/r"},"reviewDecision":"CHANGES_REQUESTED",
            "mergeable":"CONFLICTING","mergeStateStatus":"DIRTY",
            "commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"PENDING"}}}]},
            "comments":{"totalCount":0,"nodes":[]}}]}}"#;
        let data: NodesData = decode(text).unwrap();
        let prs: Vec<Pr> = data
            .nodes
            .into_iter()
            .flatten()
            .filter_map(PrNode::into_pr)
            .collect();
        assert_eq!(prs.len(), 1);
        let pr = &prs[0];
        assert_eq!(pr.key(), "o/r#3");
        assert_eq!(pr.ci, Ci::Running);
        assert_eq!(pr.review.decision, Decision::ChangesRequested);
        assert_eq!(pr.merge, Merge::Conflict);
        assert_eq!(pr.kind(), None);
    }

    #[test]
    fn errors_without_data_are_fatal() {
        let err = parse_search_response(r#"{"data":null,"errors":[{"message":"boom"}]}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("boom"), "{err}");
    }

    #[test]
    fn partial_errors_still_return_data() {
        let text = r#"{"data":{"search":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[]}},"errors":[{"message":"SAML"}]}"#;
        assert!(parse_search_response(text).unwrap().is_empty());
    }
}
