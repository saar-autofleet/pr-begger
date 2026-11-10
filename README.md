# PR Begger

Triage code-review requests by kind, from a pixel-art TUI.

Whoever opens a PR says what kind of review it needs, with one PR comment:

| kind | means |
|---|---|
| 🟩 `rubber-stamp` | mechanical change, approve as-is |
| 🤖 `ai` | let an AI agent go over it |
| 🧭 `design` | needs a human design review |

Reviewers open `pr-begger` and see every open PR that asked, grouped and
colour-coded by kind, with CI, review state, size and age next to it. Because
the signal is a comment on the PR itself, it is visible on GitHub too, not
only in the TUI.

## Install

Needs a Rust toolchain ([rustup](https://rustup.rs)) and access to this repo.

```sh
CARGO_NET_GIT_FETCH_WITH_CLI=true cargo install --git https://github.com/saar-autofleet/pr-begger
```

`CARGO_NET_GIT_FETCH_WITH_CLI=true` makes cargo clone with your `git`, which
already has your GitHub credentials (the repo is private).

### Auth

PR Begger reuses what you already have: `GITHUB_TOKEN` (or `GH_TOKEN`) if set,
otherwise the token from `gh auth token`. No new app or token. The token needs
`repo` scope to see private repos.

## Ask for a review kind

```sh
pr-begger request https://github.com/Autofleet/api/pull/812 --kind rubber-stamp
pr-begger request Autofleet/billing#431 --kind design --why "new event schema; touches every consumer"
pr-begger request Autofleet/dispatch#1290 -k ai --dry-run   # print the comment, post nothing
```

`--kind` also takes `rs`/`stamp`, `agent`/`bot`, `human`. The PR gets one
comment:

```markdown
<!-- pr-begger: {"v":1,"kind":"design","why":"new event schema; touches every consumer"} -->
🧭 **CR kind: Design review** - new event schema; touches every consumer

<sub>needs a human design review · requested with pr-begger · run `pr-begger request` again to change it</sub>
```

The hidden first line is the machine-readable marker; the rest is for people.
Asking again **edits your existing request comment** instead of adding another.
If several people's request comments are on a PR, **the newest one wins**
(newest by last edit, else by creation). You can also change the kind from the
TUI with `c`.

## The board

Run `pr-begger` with no arguments.

```text
 ███  ███     ███  ████  ███  ███ ████ ███      ██████                        ██                         ████
 █  █ █  █    █  █ █    █    █    █    █  █      ████      Rubber stamp   ██████████   AI review          ██       Design
 ███  ███     ███  ███  █ ██ █ ██ ███  ███   ████████████  3 PRs        ██████████████ 2 PRs             ██ ██     2 PRs
 █    █ █     █  █ █    █  █ █  █ █    █ █   ████████████  just approve   ██████████   bot reviews      ██    ██   humans think
 █    █  █    ███  ████  ███  ███ ████ █  █   ██ ████ ██                  ████  ████                   ██  ████ ██
 Begging 7   My PRs 3   Following 4   1 new
─── Rubber stamp 3 ────── mechanical change, approve as-is                              ┌ api#812                            × ┐
▸● stamp  api#812       Bump serde to 1.0.230 across wo… dana           +14 −14  1d ✓ ○ │                                      │
 ● stamp  infra#90      Pin terraform provider versions  you            +22 −22  1d ✓ ✓ │       ████████                       │
 ● stamp  web#77        Rename FleetCard props (no beha… omer           +48 −48  6h ◐ ○ │      ██████████                      │
─── AI review 2 ────── let an AI agent go over it                                       │       ████████        Rubber stamp   │
 ● ai     dispatch#1290 Add retry budget to the matcher… lior          +212 −40  2d ✓ ○ │         ████          mechanical     │
 ● ai     dispatch#1294 Typed config loader for routing… maya         +330 −120  9h ✗ ○ │         ████          change,        │
─── Design review 2 ────── needs a human design review                                  │   ████████████████    approve as-is  │
 ● design billing#431   Split invoice ledger into event… noa         +1840 −610  3d ✓ ✗ │  ██████████████████   asked by dana  │
 ● design infra#88      Move workers to spot node pools  you            +96 −12 21h · ○ │  ██████████████████   · 1d           │
                                                                                        │                                      │
                                                                                        │  ██  ████  ██  ████                  │
                                                                                        │                                      │
                                                                                        │ “lockfile only”                      │
                                                                                        │                                      │
                                                                                        │ Bump serde to 1.0.230 across         │
                                                                                        │ workspace                            │
                                                                                        │ acme/api                             │
                                                                                        │                                      │
                                                                                        │ author   dana                        │
                                                                                        └──────────────────────────────────────┘
 / search  ↑↓ move  ⏎ open  c beg  f filter  tab screen  w followed  p people  r refresh  ? help  q quit
```

`tab` (or `2`) switches to **My PRs**: your own open PRs in the same orgs, with
their declared kind (if any), unresolved/total review threads, review state, CI
and mergeability. PRs where someone commented, reviewed or replied in a thread
since you last looked float to the top under **NEW ACTIVITY** (bots and your
own comments do not count). Opening a PR from this screen, or pressing `x`,
marks it seen.

### Size

Where a repo's CI labels PRs with a reading-load size (`review: S`, `M`, `L`
or `XL`, as Autofleet's autorepo does: lines of added code to read, after
dropping the lockfile, fixtures, moved lines, whitespace and deletions), the
size column shows that label instead of the raw diff, because raw line counts
mislead in both directions. The detail pane shows the label with the raw
`+added −deleted` alongside. PRs without the label (other repos, or before CI
has run) show `+added −deleted` as before. The size comes from the label only,
which takes triage or write access to set; the CI's `Review:` comment is not
read. PR Begger never sets or changes the label.

```text
 ███  ███     ███  ████  ███  ███ ████ ███      ██████                        ██                         ████
 █  █ █  █    █  █ █    █    █    █    █  █      ████      Rubber stamp   ██████████   AI review          ██       Design
 ███  ███     ███  ███  █ ██ █ ██ ███  ███   ████████████  1 PR         ██████████████ 0 PRs             ██ ██     1 PR
 █    █ █     █  █ █    █  █ █  █ █    █ █   ████████████  just approve   ██████████   bot reviews      ██    ██   humans think
 █    █  █    ███  ████  ███  ███ ████ █  █   ██ ████ ██                  ████  ████                   ██  ████ ██
 Begging 7   My PRs 3   Following 4   1 new
─── New activity 1 ────── since you last looked · x marks seen                          ┌ infra#88                           × ┐
▸● design infra#88      ● Move workers to spot node …    +96 −12  ◆2/5 blocked  21h · ○ │                                      │
─── My open PRs 2 ──────                                                                │         ████                         │
 ● stamp  infra#90        Pin terraform provider ver…    +22 −22  ◇0/1 ready     1d ✓ ✓ │        ██████                        │
 ○ ?      dispatch#1301   WIP: zone-aware batching      +410 −90     – draft     4h ◐ · │         ████          Design review  │
                                                                                        │         ████          needs a human  │
                                                                                        │        ███  ██        design review  │
                                                                                        │       ███    ██       asked by you · │
                                                                                        │      ███      ██      20h            │
                                                                                        │     ███        ███                   │
                                                                                        │    ███          ██                   │
                                                                                        │   ██    ███████  █                   │
                                                                                        │                                      │
                                                                                        │ “cost vs. eviction risk tradeoff”    │
                                                                                        │                                      │
                                                                                        │ Move workers to spot node pools      │
                                                                                        │ acme/infra                           │
                                                                                        │                                      │
                                                                                        │ author   you                         │
                                                                                        │ opened   21h ago                     │
                                                                                        └──────────────────────────────────────┘
 / search  ↑↓ move  ⏎ open  c beg  f filter  tab screen  x seen  p people  r refresh  ? help  q quit
```

### Following

`tab` again (or `3`) shows **Following**: every open PR, begging or not, by
people and teams you follow, grouped by author; `v` regroups it by repo
(with an author column). On the begging board, `w`
narrows it to PRs from people you follow.

```text
 ███  ███     ███  ████  ███  ███ ████ ███      ██████                        ██                         ████
 █  █ █  █    █  █ █    █    █    █    █  █      ████      Rubber stamp   ██████████   AI review          ██       Design
 ███  ███     ███  ███  █ ██ █ ██ ███  ███   ████████████  1 PR         ██████████████ 2 PRs             ██ ██     1 PR
 █    █ █     █  █ █    █  █ █  █ █    █ █   ████████████  just approve   ██████████   bot reviews      ██    ██   humans think
 █    █  █    ███  ████  ███  ███ ████ █  █   ██ ████ ██                  ████  ████                   ██  ████ ██
 Begging 7   My PRs 3   Following 4   1 new
─── dana 1 ──────                                                                       ┌ api#812                            × ┐
▸● stamp  api#812       Bump serde to 1.0.230 across workspace          +14 −14  1d ✓ ○ │                                      │
─── lior 1 ──────                                                                       │       ████████                       │
 ● ai     dispatch#1290 Add retry budget to the matcher client         +212 −40  2d ✓ ○ │      ██████████                      │
─── maya 1 ──────                                                                       │       ████████        Rubber stamp   │
 ● ai     dispatch#1294 Typed config loader for routing service       +330 −120  9h ✗ ○ │         ████          mechanical     │
─── noa 1 ──────                                                                        │         ████          change,        │
 ● design billing#431   Split invoice ledger into events + projecti… +1840 −610  3d ✓ ✗ │   ████████████████    approve as-is  │
                                                                                        │  ██████████████████   asked by dana  │
                                                                                        │  ██████████████████   · 1d           │
                                                                                        │                                      │
                                                                                        │  ██  ████  ██  ████                  │
                                                                                        │                                      │
                                                                                        │ “lockfile only”                      │
                                                                                        │                                      │
                                                                                        │ Bump serde to 1.0.230 across         │
                                                                                        │ workspace                            │
                                                                                        │ acme/api                             │
                                                                                        │                                      │
                                                                                        │ author   dana                        │
                                                                                        └──────────────────────────────────────┘
 / search  ↑↓ move  ⏎ open  c beg  f filter  tab screen  v by repo  + follow  p people  r refresh  ? help  q quit
```

Follow from the TUI with `p` (a small window: `a` add, `d` remove), or press
`+` on any PR to follow its author. Or from the shell:

```sh
pr-begger follow alice bob carol      # people
pr-begger follow your-org/your-team   # a team (or teams/your-team, or its URL)
pr-begger unfollow alice
pr-begger following                   # who you follow
pr-begger list --following            # their open PRs, plain text
```

Following searches the same scope as the board: with `--repo X` it only
looks in repo X, so run plain `pr-begger` to see followed people's PRs across
your orgs.

Names are checked with GitHub before they are saved, so a typo is caught
right away (a login that does not exist would otherwise break the search).
Teams are expanded to their current members on every refresh. The list
lives in `~/.config/pr-begger/follow.toml`, which you can also edit by hand:

```toml
users = ["alice", "bob", "carol"]
teams = ["your-org/your-team"]
```

```text
 ███  ███     ███  ████  ███  ███ ████ ███
 █  █ █  █    █  █ █    █    █    █    █  █
 ███  ███     ███  ███  █ ██ █ ██ ███  ███
 █    █ █     █  █ █    █  █ █  █ █    █ █
 █    █  █    ███  ████  ███  ███ ████ █  █
 Begging 7   My PRs ┌ People & teams you follow                              × ┐
─── dana 1 ──────   │                                                          │
▸● stamp  api#812   │   user  dana                                             │    +14 −14  1d ✓ ○
─── lior 1 ──────   │ ▸ user  lior                                             │
 ● ai     dispatch#1│   user  maya                                             │   +212 −40  2d ✓ ○
─── maya 1 ──────   │   user  noa                                              │
 ● ai     dispatch#1│   team  acme/your-team                                   │  +330 −120  9h ✗ ○
─── noa 1 ──────    │                                                          │
 ● design billing#43│   add: carol▏                                            │ +1840 −610  3d ✓ ✗
                    │   login, org/team, teams/<team>, or a GitHub URL         │
                    │   enter add · esc cancel                                 │
                    │                                                          │
                    │                                                          │
                    └──────────────────────────────────────────────────────────┘




 / search  ↑↓ move  ⏎ open  c beg  f filter  tab screen  v by repo  + follow  p people  r refresh  ?
```

(These are the real renderer's output for demo data:
`cargo test print_screens -- --ignored --nocapture`, with `W`/`H` env vars for
the size. In a terminal the `█` pixels, dots and title bars are soft pastels;
icons are shaded with a highlight and a darker edge per material;
each pixel is a whole cell painted with a background colour, so it stays crisp
in any terminal.)

### Keys

| key | action |
|---|---|
| `/` | search every PR on all three screens with [fzf](https://github.com/junegunn/fzf): title, number (`4202` or `#4202`), repo, author, kind. `enter` jumps to it, `ctrl-o` opens it in the browser |
| `j`/`k`, `↓`/`↑` | move (`g`/`G` top/bottom, `pgup`/`pgdn`, `ctrl+d`/`ctrl+u`) |
| `enter` / `o` | open the PR in your browser |
| `c` / `b` | beg: set or change the CR kind of the selected PR (posts as you) |
| `f` / `F` | filter by kind (cycles all → stamp → ai → design) |
| `tab`, `1`, `2`, `3` | begging board / my PRs / following |
| `x` | mark a PR's activity as seen |
| `p` | people & teams you follow (`a` add, `d` remove) |
| `+` | follow the selected PR's author |
| `v` | Following: group by author / by repo |
| `w` | board: only PRs by people you follow |
| `r` | refresh now (it also refreshes every `refresh_secs`) |
| `?` | help and the glyph legend |
| `q` / `esc` | quit |

Glyphs: CI `✓` passing, `✗` failing, `◐` running, `·` none. Review `✓`
approved, `✗` changes requested, `○` waiting for reviewers. Threads `◆2/5` =
2 unresolved of 5. Status is shape first and colour second, so it reads in
monochrome too.

### Search

`/` hands every PR on all three screens to `fzf` (installed separately:
`brew install fzf`), in exact mode: every word you type must appear in the line (in
any order), so `nested` finds titles containing "nested" and nothing that
merely has those letters scattered. Type part of a title, a PR number (`4202`
or `#4202`), a repo, an author or a kind; prefix a word with `'` to match it
fuzzily, or `!` to exclude it. Rows are aligned columns (kind dot,
`repo#n`, title, author); a small preview pane under the list shows the
selected PR's full title, kind and reason, CI, review and merge state. `enter`
jumps to the PR, switching screens and clearing filters that would hide it;
`ctrl-o` opens it in the browser; `esc` goes back. Without fzf, `/` says how
to install it.

### Plain output

```sh
pr-begger list              # the board, one line per PR
pr-begger list --mine       # your PRs
pr-begger list --following  # PRs by people/teams you follow
```

Each line shows `size:M` for a PR with a `review:` size label, else the raw
`+added/-deleted`.

## Config

`~/.config/pr-begger/config.toml` (or `$XDG_CONFIG_HOME/pr-begger/`, or
`$PR_BEGGER_CONFIG`). Missing file means the defaults below. `pr-begger config`
prints the path and this example.

```toml
orgs = ["Autofleet"]          # orgs whose open PRs are searched
repos = []                    # extra owner/name repos
refresh_secs = 180            # auto-refresh while the TUI is open (0 = off)
max_prs = 200                 # cap per org/repo search
```

**Which repos.** By default every repo in the `Autofleet` org is covered, so
`Autofleet/autorepo` is already on the board, in My PRs and in Following. Add
`repos = ["someone/else"]` for repos outside your orgs. `--org X` and
`--repo owner/name` (both repeatable) *replace* the file's scope for one run,
e.g. `pr-begger --repo Autofleet/autorepo` shows only that repo.

## How it works

- **Finding requests.** One GitHub search per org/repo, run in parallel:
  `is:pr is:open archived:false "pr-begger" in:comments org:X`. The visible
  word `pr-begger` in every request comment is what the search matches (the
  search index is not guaranteed to cover the hidden HTML comment). A match
  only counts if one of the PR's last 40 comments has a readable marker.
  Repos named with `repos`/`--repo` skip the search: their open PRs are listed
  directly, so new requests show up there immediately.
- **Search lag.** GitHub's search index can take a few minutes to pick up a
  new comment, so in an org-wide board someone else's brand-new request may
  appear on a later refresh. Requests you make from the TUI show up at once.
- **Fast.** The search asks for PR ids only. Everything a row shows (comments,
  labels, CI rollup, review decision, mergeability, threads) is then fetched
  by id in small batches, up to 8 in parallel. GitHub resolves those nested fields one
  PR at a time, so asking for them in the search made a 40-PR "My PRs" fetch
  take ~7–27s; batched it takes ~2–6s. A batch GitHub times out on (502) is
  split in half and retried.
- **Snappy startup.** The last board is cached in
  `~/.cache/pr-begger/board.json` and drawn immediately while the refresh runs.
  "Last looked" times live in `~/.local/state/pr-begger/seen.json`.
- **Just-posted requests** stay on the board for a few minutes even if
  GitHub's search index has not caught up with the new comment yet.
- Set `PR_BEGGER_DEBUG=1` to log each GitHub request's status, time and size
  to stderr.

## Limits

- A marker older than a PR's last 40 comments is not seen. Asking again
  (which edits or posts a fresh comment) brings it back.
- GitHub search returns at most 1000 results per query; `max_prs` caps it lower.
- Org-wide boards depend on GitHub's search index (see "Search lag" above).

## Follow-ups (not in v1)

- Slack: post or mirror requests into the team / R&D CR channels.
- GitHub labels as an alternative or mirror of the comment signal.
- Approving (rubber-stamping) straight from the TUI.
- Triggering the AI review agent for `ai` PRs.
- Per-team views (e.g. only PRs from my team's members) and reviewer
  assignment.

## Development

```sh
cargo test                       # marker parse/serialize, newest-wins, config, rendering
cargo clippy --all-targets -- -D warnings
cargo fmt
cargo run -- --repo saar-autofleet/pr-begger
```

Tests never touch the network: GitHub responses are recorded fixtures in
`tests/fixtures/`. When trying the posting path for real, use a PR in this
repo, not a team repo.

## Credits

- Ideas (not code) from [barspielberg/pr-pile](https://github.com/barspielberg/pr-pile),
  a Go PR board: GraphQL over HTTP instead of shelling out to `gh` per call,
  the token from `gh auth token`, one parallel search per scope, CI rollup
  asked for on its own, status glyphs that work without colour, and ANSI
  theme colours for text so it reads on light and dark terminals.
- The 16-colour palette is the PICO-8 one.
