//! `ready-e2e` and `rerun-sim`: nudge a PR's e2e CI without the Actions UI.
//!
//! Both act on autorepo-style CI (names come from the `[ci]` config, see
//! `config::CiConfig`). There, `main_ci.yml` runs every e2e suite after one job,
//! `Setup shared simulation`, which only starts when `check_e2e_label` found
//! the `ready-for-e2e` label. GitHub's job re-run endpoint re-runs a job *and
//! every job downstream of it*, which gives the two re-run points:
//!
//! - **ready-e2e** adds the label, then re-runs `check_e2e_label`. That is the
//!   job that failed, it reads the PR's labels live, and everything gated on
//!   it (the shared simulation, then the suites) follows it. Re-running only
//!   the simulation would not do: its `if:` reads `check_e2e_label`'s output,
//!   which stays `not-found` until that job itself runs again. This is also
//!   what the job's own failure message asks for ("add the label, then re-run
//!   the failed jobs"), minus any unrelated failed jobs.
//! - **rerun-sim** re-runs `Setup shared simulation`, the job `main_ci.yml`
//!   documents as the re-run point for e2e flakes: on a re-run it guards
//!   against stale runs, resets the platform e2e status, and the suites that
//!   already passed skip themselves.
//!
//! Each action first builds a [`Plan`] from read-only calls (that is all
//! `--dry-run` does), then [`execute`] carries it out. A run that is still in
//! progress is refused rather than queued behind: GitHub cannot re-run part
//! of a live run, and a second simulation would be a duplicate.

use std::fmt;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::CiConfig;
use crate::github::Client;
use crate::target::PrRef;

/// Most jobs fetched per run (10 pages of 100); autorepo's CI has far fewer.
const MAX_JOB_PAGES: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    ReadyForE2e,
    RerunSim,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::ReadyForE2e => "ready for e2e",
            Action::RerunSim => "re-run shared simulation",
        }
    }
}

/// The PR facts the actions need (`GET /repos/{o}/{r}/pulls/{n}`).
#[derive(Debug, Clone, Deserialize)]
pub struct PrHead {
    pub html_url: String,
    pub state: String,
    #[serde(default)]
    pub merged: bool,
    pub head: Head,
    #[serde(default)]
    pub labels: Vec<Label>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Head {
    pub sha: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Label {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Run {
    pub id: u64,
    #[serde(default)]
    pub name: Option<String>,
    pub status: Option<String>,
    #[serde(default)]
    pub run_number: u64,
    #[serde(default = "one")]
    pub run_attempt: u64,
    #[serde(default)]
    pub html_url: String,
}

fn one() -> u64 {
    1
}

impl Run {
    fn completed(&self) -> bool {
        self.status.as_deref() == Some("completed")
    }

    fn describe(&self) -> String {
        let attempt = if self.run_attempt > 1 {
            format!(" attempt {}", self.run_attempt)
        } else {
            String::new()
        };
        format!("run #{}{attempt}", self.run_number)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Job {
    pub id: u64,
    pub name: String,
    pub status: Option<String>,
    pub conclusion: Option<String>,
    #[serde(default)]
    pub html_url: String,
}

#[derive(Deserialize)]
struct RunsPage {
    #[serde(default)]
    workflow_runs: Vec<Run>,
}

#[derive(Deserialize)]
struct JobsPage {
    #[serde(default)]
    total_count: usize,
    #[serde(default)]
    jobs: Vec<Job>,
}

pub fn parse_runs(v: Value) -> Result<Vec<Run>> {
    Ok(serde_json::from_value::<RunsPage>(v)
        .context("decoding workflow runs")?
        .workflow_runs)
}

/// One page of jobs and the run's total job count.
pub fn parse_jobs(v: Value) -> Result<(Vec<Job>, usize)> {
    let page: JobsPage = serde_json::from_value(v).context("decoding jobs")?;
    Ok((page.jobs, page.total_count))
}

/// The GitHub calls the actions make; a trait so tests can fake GitHub.
pub trait Actions {
    fn pr_head(&self, pr: &PrRef) -> Result<PrHead>;
    fn runs_for(&self, pr: &PrRef, sha: &str) -> Result<Vec<Run>>;
    fn jobs(&self, pr: &PrRef, run_id: u64) -> Result<Vec<Job>>;
    fn add_label(&self, pr: &PrRef, label: &str) -> Result<()>;
    fn rerun_job(&self, pr: &PrRef, job_id: u64) -> Result<()>;
}

impl Actions for Client {
    fn pr_head(&self, pr: &PrRef) -> Result<PrHead> {
        let v = self.rest(
            "GET",
            &format!("/repos/{}/{}/pulls/{}", pr.owner, pr.repo, pr.number),
            None,
        )?;
        serde_json::from_value(v).context("decoding the PR")
    }

    fn runs_for(&self, pr: &PrRef, sha: &str) -> Result<Vec<Run>> {
        parse_runs(self.rest(
            "GET",
            &format!(
                "/repos/{}/{}/actions/runs?head_sha={sha}&per_page=100",
                pr.owner, pr.repo
            ),
            None,
        )?)
    }

    fn jobs(&self, pr: &PrRef, run_id: u64) -> Result<Vec<Job>> {
        let mut out = Vec::new();
        for page in 1..=MAX_JOB_PAGES {
            let (jobs, total) = parse_jobs(self.rest(
                "GET",
                &format!(
                    "/repos/{}/{}/actions/runs/{run_id}/jobs?filter=latest&per_page=100&page={page}",
                    pr.owner, pr.repo
                ),
                None,
            )?)?;
            let empty = jobs.is_empty();
            out.extend(jobs);
            if empty || out.len() >= total {
                break;
            }
        }
        Ok(out)
    }

    fn add_label(&self, pr: &PrRef, label: &str) -> Result<()> {
        self.rest(
            "POST",
            &format!(
                "/repos/{}/{}/issues/{}/labels",
                pr.owner, pr.repo, pr.number
            ),
            Some(&json!({ "labels": [label] })),
        )?;
        Ok(())
    }

    fn rerun_job(&self, pr: &PrRef, job_id: u64) -> Result<()> {
        self.rest(
            "POST",
            &format!(
                "/repos/{}/{}/actions/jobs/{job_id}/rerun",
                pr.owner, pr.repo
            ),
            None,
        )?;
        Ok(())
    }
}

/// `want` names a job by its own name: it matches that name alone or as the
/// last ` / ` segment, so `check_e2e_label` finds
/// `call_apps_ci / check_e2e_label` (jobs of a called workflow carry the
/// caller's job id as a prefix).
pub fn job_matches(name: &str, want: &str) -> bool {
    name == want || name.rsplit(" / ").next() == Some(want)
}

pub fn find_job<'a>(jobs: &'a [Job], want: &str) -> Option<&'a Job> {
    jobs.iter().find(|j| job_matches(&j.name, want))
}

/// The newest run of `workflow` (re-runs keep their run id, so the newest id
/// is the latest push's run).
pub fn pick_run<'a>(runs: &'a [Run], workflow: &str) -> Option<&'a Run> {
    runs.iter()
        .filter(|r| r.name.as_deref() == Some(workflow))
        .max_by_key(|r| r.id)
}

/// A job to re-run, and where it lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rerun {
    pub job_id: u64,
    pub job_name: String,
    pub job_url: String,
    pub run: String,
}

/// What an action would do, from read-only calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub action: Action,
    pub pr_url: String,
    pub sha: String,
    /// Label to add (absent: nothing to add).
    pub add_label: Option<String>,
    /// Label already on the PR.
    pub has_label: Option<String>,
    pub rerun: Option<Rerun>,
    /// Why there is no re-run, when there is none.
    pub note: Option<String>,
    /// The note is a refusal: after any labelling, executing fails with it.
    pub refused: bool,
}

impl Plan {
    /// Something to do (a label to add or a job to re-run).
    pub fn acts(&self) -> bool {
        self.add_label.is_some() || self.rerun.is_some()
    }

    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![format!(
            "{} (head {})",
            self.pr_url,
            &self.sha[..self.sha.len().min(7)]
        )];
        if let Some(l) = &self.add_label {
            out.push(format!("add the `{l}` label"));
        }
        if let Some(l) = &self.has_label {
            out.push(format!("`{l}` label already on the PR"));
        }
        if let Some(r) = &self.rerun {
            out.push(format!(
                "re-run `{}` in {} (and every job after it)",
                r.job_name, r.run
            ));
            if !r.job_url.is_empty() {
                out.push(format!("  {}", r.job_url));
            }
        }
        if let Some(n) = &self.note {
            out.push(n.clone());
        }
        out
    }
}

impl fmt::Display for Plan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for l in self.lines() {
            writeln!(f, "{l}")?;
        }
        Ok(())
    }
}

fn open_head(api: &dyn Actions, pr: &PrRef) -> Result<PrHead> {
    let head = api.pr_head(pr).with_context(|| format!("reading {pr}"))?;
    if head.state != "open" {
        let state = if head.merged { "merged" } else { "closed" };
        bail!("{} is {state}, not open", head.html_url);
    }
    Ok(head)
}

/// The newest `workflow` run for the head commit and its jobs.
fn locate(
    api: &dyn Actions,
    cfg: &CiConfig,
    pr: &PrRef,
    sha: &str,
) -> Result<Option<(Run, Vec<Job>)>> {
    let runs = api
        .runs_for(pr, sha)
        .with_context(|| format!("listing workflow runs for {pr}"))?;
    let Some(run) = pick_run(&runs, &cfg.workflow).cloned() else {
        return Ok(None);
    };
    let jobs = api
        .jobs(pr, run.id)
        .with_context(|| format!("listing the jobs of {}", run.describe()))?;
    Ok(Some((run, jobs)))
}

fn rerun_of(job: &Job, run: &Run) -> Rerun {
    Rerun {
        job_id: job.id,
        job_name: job.name.clone(),
        job_url: job.html_url.clone(),
        run: run.describe(),
    }
}

pub fn plan(api: &dyn Actions, cfg: &CiConfig, pr: &PrRef, action: Action) -> Result<Plan> {
    match action {
        Action::ReadyForE2e => plan_ready(api, cfg, pr),
        Action::RerunSim => plan_rerun_sim(api, cfg, pr),
    }
}

/// Add the label, then re-run the label check when it already failed.
pub fn plan_ready(api: &dyn Actions, cfg: &CiConfig, pr: &PrRef) -> Result<Plan> {
    let head = open_head(api, pr)?;
    let label = &cfg.e2e_label;
    let has = head.labels.iter().any(|l| &l.name == label);
    let mut plan = Plan {
        action: Action::ReadyForE2e,
        pr_url: head.html_url.clone(),
        sha: head.head.sha.clone(),
        add_label: (!has).then(|| label.clone()),
        has_label: has.then(|| label.clone()),
        rerun: None,
        note: None,
        refused: false,
    };
    let want = &cfg.label_check_job;
    let (note, refused) = match locate(api, cfg, pr, &head.head.sha)? {
        None => (
            format!(
                "no `{}` run for the head commit yet; when it runs, `{want}` sees the label",
                cfg.workflow
            ),
            false,
        ),
        Some((run, jobs)) => match find_job(&jobs, want) {
            None if run.completed() => (
                format!(
                    "{} has no `{want}` job: e2e does not run for this PR",
                    run.describe()
                ),
                false,
            ),
            None => (
                format!(
                    "{} is running and has not reached `{want}` yet; it will see the label",
                    run.describe()
                ),
                false,
            ),
            Some(job) => match (job.status.as_deref(), job.conclusion.as_deref()) {
                (Some("completed"), Some("success")) => (
                    format!(
                        "`{}` already passed in {}; nothing to re-run",
                        job.name,
                        run.describe()
                    ),
                    false,
                ),
                (Some("completed"), Some("skipped")) => (
                    format!(
                        "`{}` was skipped in {}: e2e does not run for this PR (or earlier jobs failed)",
                        job.name,
                        run.describe()
                    ),
                    false,
                ),
                (Some("completed"), _) if !run.completed() => (
                    format!(
                        "`{}` failed but {} is still in progress; re-run once it finishes",
                        job.name,
                        run.describe()
                    ),
                    true,
                ),
                (Some("completed"), _) => {
                    plan.rerun = Some(rerun_of(job, &run));
                    return Ok(plan);
                }
                _ => (
                    format!(
                        "`{}` has not finished in {}; it will see the label",
                        job.name,
                        run.describe()
                    ),
                    false,
                ),
            },
        },
    };
    plan.note = Some(note);
    plan.refused = refused;
    Ok(plan)
}

/// Re-run the shared-simulation job of the head commit's finished CI run.
pub fn plan_rerun_sim(api: &dyn Actions, cfg: &CiConfig, pr: &PrRef) -> Result<Plan> {
    let head = open_head(api, pr)?;
    let want = &cfg.shared_sim_job;
    let (run, jobs) = locate(api, cfg, pr, &head.head.sha)?.ok_or_else(|| {
        anyhow!(
            "no `{}` run for the head commit of {}",
            cfg.workflow,
            head.html_url
        )
    })?;
    if !run.completed() {
        bail!(
            "{} is still in progress; wait for it to finish (re-running now would start a second simulation): {}",
            run.describe(),
            run.html_url
        );
    }
    let job = find_job(&jobs, want).ok_or_else(|| {
        anyhow!(
            "{} has no `{want}` job: e2e does not run for this PR",
            run.describe()
        )
    })?;
    if job.conclusion.as_deref() == Some("skipped") {
        bail!(
            "`{}` was skipped in {}: the `{}` label is missing (use ready-e2e) or e2e does not run for this PR",
            job.name,
            run.describe(),
            cfg.e2e_label
        );
    }
    Ok(Plan {
        action: Action::RerunSim,
        pr_url: head.html_url,
        sha: head.head.sha,
        add_label: None,
        has_label: None,
        rerun: Some(rerun_of(job, &run)),
        note: None,
        refused: false,
    })
}

/// Carry out a plan. Returns a one-line summary.
pub fn execute(api: &dyn Actions, pr: &PrRef, plan: &Plan) -> Result<String> {
    let mut done = Vec::new();
    if let Some(label) = &plan.add_label {
        api.add_label(pr, label)
            .with_context(|| format!("adding the `{label}` label to {pr}"))?;
        done.push(format!("added `{label}`"));
    }
    if plan.refused {
        let note = plan.note.as_deref().unwrap_or("refused");
        if done.is_empty() {
            bail!("{note}");
        }
        bail!("{}, but {note}", done.join(", "));
    }
    if let Some(r) = &plan.rerun {
        api.rerun_job(pr, r.job_id)
            .with_context(|| format!("re-running `{}` in {}", r.job_name, r.run))?;
        done.push(format!("re-running `{}` in {}", r.job_name, r.run));
    }
    if done.is_empty() {
        return Ok(format!(
            "nothing to do: {}",
            plan.note.as_deref().unwrap_or("")
        ));
    }
    let mut out = format!("{pr}: {}", done.join(", "));
    if plan.rerun.is_none() {
        if let Some(n) = &plan.note {
            out.push_str(&format!(" ({n})"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn job(id: u64, name: &str, status: &str, conclusion: Option<&str>) -> Job {
        Job {
            id,
            name: name.into(),
            status: Some(status.into()),
            conclusion: conclusion.map(String::from),
            html_url: format!("https://github.com/o/r/actions/runs/1/job/{id}"),
        }
    }

    fn run(id: u64, name: &str, status: &str) -> Run {
        Run {
            id,
            name: Some(name.into()),
            status: Some(status.into()),
            run_number: id,
            run_attempt: 1,
            html_url: format!("https://github.com/o/r/actions/runs/{id}"),
        }
    }

    const WF: &str = "Detect Affected Projects";

    /// A fake GitHub: canned answers, and a log of every write.
    struct Fake {
        state: &'static str,
        labels: Vec<&'static str>,
        runs: Vec<Run>,
        jobs: Vec<Job>,
        write_err: Option<&'static str>,
        workflow: &'static str,
        writes: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new(runs: Vec<Run>, jobs: Vec<Job>) -> Self {
            Fake {
                state: "open",
                labels: vec![],
                runs,
                jobs,
                write_err: None,
                workflow: WF,
                writes: RefCell::new(vec![]),
            }
        }

        fn writes(&self) -> Vec<String> {
            self.writes.borrow().clone()
        }
    }

    impl Actions for Fake {
        fn pr_head(&self, _: &PrRef) -> Result<PrHead> {
            Ok(PrHead {
                html_url: "https://github.com/o/r/pull/7".into(),
                state: self.state.into(),
                merged: false,
                head: Head {
                    sha: "abcdef1234567".into(),
                },
                labels: self
                    .labels
                    .iter()
                    .map(|l| Label {
                        name: l.to_string(),
                    })
                    .collect(),
            })
        }
        fn runs_for(&self, _: &PrRef, sha: &str) -> Result<Vec<Run>> {
            assert_eq!(sha, "abcdef1234567");
            Ok(self.runs.clone())
        }
        fn jobs(&self, _: &PrRef, run_id: u64) -> Result<Vec<Job>> {
            assert_eq!(
                Some(run_id),
                pick_run(&self.runs, self.workflow).map(|r| r.id)
            );
            Ok(self.jobs.clone())
        }
        fn add_label(&self, _: &PrRef, label: &str) -> Result<()> {
            if let Some(e) = self.write_err {
                return Err(crate::github::rest_error(403, e).unwrap());
            }
            self.writes.borrow_mut().push(format!("label {label}"));
            Ok(())
        }
        fn rerun_job(&self, _: &PrRef, job_id: u64) -> Result<()> {
            if let Some(e) = self.write_err {
                return Err(crate::github::rest_error(403, e).unwrap());
            }
            self.writes.borrow_mut().push(format!("rerun {job_id}"));
            Ok(())
        }
    }

    fn pr() -> PrRef {
        "o/r#7".parse().unwrap()
    }

    fn cfg() -> CiConfig {
        CiConfig::default()
    }

    /// The jobs of a finished autorepo PR run that lacked the label.
    fn unlabelled_jobs() -> Vec<Job> {
        vec![
            job(1, "Analyze Changed Files", "completed", Some("success")),
            job(
                2,
                "call_apps_ci / lint-typecheck-test ordering-ms",
                "completed",
                Some("success"),
            ),
            job(
                3,
                "call_apps_ci / check_e2e_label",
                "completed",
                Some("failure"),
            ),
            job(
                4,
                "call_apps_ci / Setup shared simulation",
                "completed",
                Some("skipped"),
            ),
        ]
    }

    fn run_and_execute(f: &Fake, action: Action) -> Result<String> {
        let p = plan(f, &cfg(), &pr(), action)?;
        execute(f, &pr(), &p)
    }

    #[test]
    fn jobs_match_by_last_segment() {
        assert!(job_matches("check_e2e_label", "check_e2e_label"));
        assert!(job_matches(
            "call_apps_ci / Setup shared simulation",
            "Setup shared simulation"
        ));
        assert!(job_matches("a / b / check_e2e_label", "check_e2e_label"));
        assert!(!job_matches(
            "call_apps_ci / check_e2e_label_v2",
            "check_e2e_label"
        ));
        assert!(!job_matches(
            "call_apps_ci / Cleanup Shared Simulation",
            "Setup shared simulation"
        ));
    }

    #[test]
    fn picks_the_newest_run_of_the_ci_workflow() {
        let runs = vec![
            run(10, WF, "completed"),
            run(30, "Claude Review", "completed"),
            run(20, WF, "completed"),
        ];
        assert_eq!(pick_run(&runs, WF).unwrap().id, 20);
        assert!(pick_run(&runs, "Nope").is_none());
    }

    #[test]
    fn parses_recorded_runs_and_jobs() {
        let runs = parse_runs(serde_json::json!({
            "total_count": 1,
            "workflow_runs": [{"id": 5, "name": WF, "status": "completed",
                "conclusion": "failure", "run_number": 812, "run_attempt": 2,
                "html_url": "https://github.com/o/r/actions/runs/5", "extra": 1}]
        }))
        .unwrap();
        assert_eq!(runs[0].describe(), "run #812 attempt 2");
        let (jobs, total) = parse_jobs(serde_json::json!({
            "total_count": 2,
            "jobs": [{"id": 9, "name": "call_apps_ci / check_e2e_label",
                "status": "completed", "conclusion": null, "html_url": "u"}]
        }))
        .unwrap();
        assert_eq!((jobs[0].id, total), (9, 2));
        assert!(jobs[0].conclusion.is_none());
    }

    #[test]
    fn ready_labels_then_reruns_the_failed_label_check() {
        let f = Fake::new(vec![run(5, WF, "completed")], unlabelled_jobs());
        let p = plan(&f, &cfg(), &pr(), Action::ReadyForE2e).unwrap();
        assert_eq!(p.add_label.as_deref(), Some("ready-for-e2e"));
        assert_eq!(p.rerun.as_ref().unwrap().job_id, 3);
        assert!(p
            .to_string()
            .contains("re-run `call_apps_ci / check_e2e_label` in run #5"));
        assert!(f.writes().is_empty(), "planning must not write");
        let out = execute(&f, &pr(), &p).unwrap();
        assert_eq!(f.writes(), vec!["label ready-for-e2e", "rerun 3"]);
        assert!(out.contains("added `ready-for-e2e`"), "{out}");
    }

    #[test]
    fn ready_with_label_present_only_reruns() {
        let mut f = Fake::new(vec![run(5, WF, "completed")], unlabelled_jobs());
        f.labels = vec!["ready-for-e2e"];
        let p = plan(&f, &cfg(), &pr(), Action::ReadyForE2e).unwrap();
        assert!(p.add_label.is_none());
        assert!(p.to_string().contains("already on the PR"));
        execute(&f, &pr(), &p).unwrap();
        assert_eq!(f.writes(), vec!["rerun 3"]);
    }

    #[test]
    fn ready_with_nothing_to_do_does_nothing() {
        let mut jobs = unlabelled_jobs();
        jobs[2].conclusion = Some("success".into());
        let mut f = Fake::new(vec![run(5, WF, "completed")], jobs);
        f.labels = vec!["ready-for-e2e"];
        let p = plan(&f, &cfg(), &pr(), Action::ReadyForE2e).unwrap();
        assert!(!p.acts());
        assert!(p.note.as_ref().unwrap().contains("already passed"));
        let out = execute(&f, &pr(), &p).unwrap();
        assert!(out.starts_with("nothing to do"), "{out}");
        assert!(f.writes().is_empty());
    }

    #[test]
    fn ready_without_a_run_only_labels() {
        let f = Fake::new(vec![run(9, "Claude Review", "completed")], vec![]);
        let out = run_and_execute(&f, Action::ReadyForE2e).unwrap();
        assert_eq!(f.writes(), vec!["label ready-for-e2e"]);
        assert!(out.contains("no `Detect Affected Projects` run"), "{out}");
    }

    #[test]
    fn ready_while_the_check_is_pending_only_labels() {
        let mut jobs = unlabelled_jobs();
        jobs[2].status = Some("queued".into());
        jobs[2].conclusion = None;
        let f = Fake::new(vec![run(5, WF, "in_progress")], jobs);
        let out = run_and_execute(&f, Action::ReadyForE2e).unwrap();
        assert_eq!(f.writes(), vec!["label ready-for-e2e"]);
        assert!(out.contains("will see the label"), "{out}");
    }

    #[test]
    fn ready_refuses_to_rerun_inside_a_live_run_but_still_labels() {
        let f = Fake::new(vec![run(5, WF, "in_progress")], unlabelled_jobs());
        let e = run_and_execute(&f, Action::ReadyForE2e)
            .unwrap_err()
            .to_string();
        assert_eq!(f.writes(), vec!["label ready-for-e2e"]);
        assert!(e.contains("added `ready-for-e2e`, but"), "{e}");
        assert!(e.contains("still in progress"), "{e}");
    }

    #[test]
    fn ready_without_the_job_explains() {
        let f = Fake::new(
            vec![run(5, WF, "completed")],
            unlabelled_jobs()[..2].to_vec(),
        );
        let p = plan(&f, &cfg(), &pr(), Action::ReadyForE2e).unwrap();
        assert!(p.rerun.is_none());
        assert!(p.note.unwrap().contains("e2e does not run"));
    }

    #[test]
    fn closed_prs_are_refused() {
        let mut f = Fake::new(vec![run(5, WF, "completed")], unlabelled_jobs());
        f.state = "closed";
        for a in [Action::ReadyForE2e, Action::RerunSim] {
            let e = plan(&f, &cfg(), &pr(), a).unwrap_err().to_string();
            assert!(e.contains("is closed, not open"), "{e}");
        }
    }

    #[test]
    fn rerun_sim_reruns_the_sim_job_of_the_newest_run() {
        let mut jobs = unlabelled_jobs();
        jobs[2].conclusion = Some("success".into());
        jobs[3].conclusion = Some("success".into());
        let f = Fake::new(vec![run(4, WF, "completed"), run(5, WF, "completed")], jobs);
        let out = run_and_execute(&f, Action::RerunSim).unwrap();
        assert_eq!(f.writes(), vec!["rerun 4"]);
        assert!(
            out.contains("re-running `call_apps_ci / Setup shared simulation` in run #5"),
            "{out}"
        );
    }

    #[test]
    fn rerun_sim_error_paths() {
        let mut ok_jobs = unlabelled_jobs();
        ok_jobs[3].conclusion = Some("failure".into());
        let cases: Vec<(Fake, &str)> = vec![
            (
                Fake::new(vec![], vec![]),
                "no `Detect Affected Projects` run",
            ),
            (
                Fake::new(vec![run(5, WF, "in_progress")], ok_jobs.clone()),
                "still in progress",
            ),
            (
                Fake::new(vec![run(5, WF, "completed")], ok_jobs[..3].to_vec()),
                "has no `Setup shared simulation` job",
            ),
            (
                Fake::new(vec![run(5, WF, "completed")], unlabelled_jobs()),
                "label is missing (use ready-e2e)",
            ),
        ];
        for (f, want) in cases {
            let e = run_and_execute(&f, Action::RerunSim)
                .unwrap_err()
                .to_string();
            assert!(e.contains(want), "{e} should mention {want}");
            assert!(f.writes().is_empty());
        }
    }

    #[test]
    fn permission_errors_surface_with_context() {
        let mut jobs = unlabelled_jobs();
        jobs[3].conclusion = Some("failure".into());
        let mut f = Fake::new(vec![run(5, WF, "completed")], jobs);
        f.write_err = Some(r#"{"message":"Must have admin rights to Repository."}"#);
        let e = run_and_execute(&f, Action::RerunSim).unwrap_err();
        let msg = format!("{e:#}");
        assert!(
            msg.contains("re-running `call_apps_ci / Setup shared simulation`"),
            "{msg}"
        );
        assert!(msg.contains("permission denied (403)"), "{msg}");
        let e = run_and_execute(&f, Action::ReadyForE2e).unwrap_err();
        assert!(format!("{e:#}").contains("adding the `ready-for-e2e` label"));
    }

    #[test]
    fn configured_names_are_used() {
        let mut f = Fake::new(
            vec![run(5, "CI", "completed"), run(6, WF, "completed")],
            vec![job(8, "e2e-gate", "completed", Some("failure"))],
        );
        f.workflow = "CI";
        let cfg = CiConfig {
            workflow: "CI".into(),
            e2e_label: "e2e-please".into(),
            label_check_job: "e2e-gate".into(),
            shared_sim_job: "sim".into(),
        };
        let p = plan(&f, &cfg, &pr(), Action::ReadyForE2e).unwrap();
        execute(&f, &pr(), &p).unwrap();
        assert_eq!(f.writes(), vec!["label e2e-please", "rerun 8"]);
    }
}
