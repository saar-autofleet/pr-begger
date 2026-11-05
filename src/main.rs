//! PR Begger: triage code-review requests by kind from a pixel-art TUI.

mod config;
mod follow;
mod github;
mod kind;
mod marker;
mod model;
mod request;
mod store;
mod target;
mod tui;

use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};

use crate::config::Config;
use crate::kind::Kind;
use crate::model::{age, Pr};
use crate::target::PrRef;

#[derive(Parser)]
#[command(
    name = "pr-begger",
    version,
    about = "Beg for the right kind of code review, and triage everyone else's begging.",
    long_about = "Run with no command to open the TUI board of open PRs that asked for a \
                  review kind (rubber-stamp / ai / design), plus a 'My PRs' screen."
)]
struct Cli {
    /// Search this org (repeatable). Replaces the config file's orgs and repos.
    #[arg(long, global = true, value_name = "ORG")]
    org: Vec<String>,
    /// Search this owner/name repo (repeatable). Replaces the config file's orgs and repos.
    #[arg(long, global = true, value_name = "OWNER/NAME")]
    repo: Vec<String>,
    /// Config file to use instead of ~/.config/pr-begger/config.toml.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Ask for a kind of review on a PR (posts or updates one PR comment).
    Request {
        /// PR URL or owner/repo#number.
        pr: PrRef,
        /// rubber-stamp (rs) | ai | design
        #[arg(long, short)]
        kind: Kind,
        /// One line on why, shown to reviewers.
        #[arg(long, short)]
        why: Option<String>,
        /// Print the comment instead of posting it.
        #[arg(long)]
        dry_run: bool,
    },
    /// Print the board as plain text (for scripts and quick looks).
    List {
        /// List your own open PRs instead of the begging board.
        #[arg(long, conflicts_with = "following")]
        mine: bool,
        /// List open PRs by the people and teams you follow.
        #[arg(long)]
        following: bool,
    },
    /// Follow people or teams: `login`, `org/team`, `teams/team` (first
    /// configured org) or a GitHub URL. Their open PRs show on the
    /// Following screen.
    Follow {
        #[arg(required = true)]
        who: Vec<String>,
    },
    /// Stop following people or teams.
    Unfollow {
        #[arg(required = true)]
        who: Vec<String>,
    },
    /// List who you follow.
    Following,
    /// Show the config file path and an example config.
    Config,
}

fn config_file(cli: &Cli) -> Option<PathBuf> {
    cli.config.clone().or_else(config::config_path)
}

fn follow_file(cli: &Cli) -> follow::FollowFile {
    follow::FollowFile::new(config::follow_path(config_file(cli).as_deref()))
}

fn load_config(cli: &Cli) -> Result<Config> {
    let path = config_file(cli);
    let cfg = match &path {
        Some(p) => Config::load(p)?,
        None => Config::default(),
    };
    cfg.with_overrides(&cli.org, &cli.repo)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        None => {
            let cfg = load_config(&cli)?;
            tui::run(cfg, follow_file(&cli))
        }
        Some(Cmd::Request {
            pr,
            kind,
            why,
            dry_run,
        }) => {
            if *dry_run {
                print!(
                    "{}",
                    marker::Marker::new(*kind, why.as_deref()).to_comment_body()
                );
                return Ok(());
            }
            let client = github::Client::from_env()?;
            let out = request::request(&client, pr, *kind, why.as_deref())
                .with_context(|| format!("requesting a {} review on {pr}", kind.label()))?;
            let verb = if out.updated { "Updated" } else { "Posted" };
            println!(
                "{verb} {} {} request: {}",
                kind.emoji(),
                kind.label(),
                out.url
            );
            Ok(())
        }
        Some(Cmd::List { mine, following }) => {
            let cfg = load_config(&cli)?;
            let client = github::Client::from_env()?;
            let mut prs = if *mine {
                client.fetch_mine(&cfg)?
            } else if *following {
                let follows = follow_file(&cli).load()?;
                client.fetch_following(&cfg, &follows)?.0
            } else {
                client.fetch_board(&cfg)?
            };
            tui::sort_prs(&mut prs);
            print_list(&prs);
            Ok(())
        }
        Some(Cmd::Follow { who }) => {
            let cfg = load_config(&cli)?;
            let file = follow_file(&cli);
            let mut follows = file.load()?;
            let client = github::Client::from_env()?;
            for w in who {
                let parsed = follow::Who::parse(w, cfg.orgs.first().map(String::as_str))?;
                let (canonical, desc) = client.check(&parsed)?;
                if follows.add(&canonical) {
                    println!("following {desc}");
                } else {
                    println!("already following {canonical}");
                }
            }
            file.save(&follows)?;
            Ok(())
        }
        Some(Cmd::Unfollow { who }) => {
            let cfg = load_config(&cli)?;
            let file = follow_file(&cli);
            let mut follows = file.load()?;
            for w in who {
                let parsed = follow::Who::parse(w, cfg.orgs.first().map(String::as_str))?;
                if follows.remove(&parsed) {
                    println!("unfollowed {parsed}");
                } else {
                    println!("was not following {parsed}");
                }
            }
            file.save(&follows)?;
            Ok(())
        }
        Some(Cmd::Following) => {
            let file = follow_file(&cli);
            let follows = file.load()?;
            if let Some(p) = file.path() {
                println!("# {}", p.display());
            }
            if follows.is_empty() {
                println!("not following anyone yet: pr-begger follow <login|org/team>");
            }
            for w in follows.all() {
                match w {
                    follow::Who::User(u) => println!("user  {u}"),
                    team => println!("team  {team}"),
                }
            }
            Ok(())
        }
        Some(Cmd::Config) => {
            match cli.config.clone().or_else(config::config_path) {
                Some(p) => println!("config file: {}", p.display()),
                None => println!("config file: (no HOME; using defaults)"),
            }
            println!("\n{}", config::EXAMPLE);
            Ok(())
        }
    }
}

fn print_list(prs: &[Pr]) {
    let now = Utc::now();
    if prs.is_empty() {
        println!("nothing here");
    }
    for pr in prs {
        let kind = pr.kind().map_or("-", Kind::id);
        println!(
            "{:<12} {:<28} {:>4} +{}/-{} ci:{} review:{} merge:{}  {}  ({})",
            kind,
            pr.key(),
            age(pr.created_at, now),
            pr.additions,
            pr.deletions,
            pr.ci.word(),
            pr.review.word(),
            pr.merge.word(),
            pr.title,
            pr.url,
        );
    }
}
