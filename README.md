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

 █▀▀▄ █▀▀▄    █▀▀▄ █▀▀▀ ▄▀▀▀ ▄▀▀▀ █▀▀▀ █▀▀▄  █▀▀▀▀▀▀▀▀█              █▀▀▀▀▀▀▀▀█              █▀▀▀██▀▀▀█
 ▀▀▀▀ ▀▀▀▀    ▀▀▀▀ ▀▀▀  ▀ ▀▀ ▀ ▀▀ ▀▀▀  ▀▀▀▀  ██▀▀▀▀▀▀██ RUBBER STAMP ██▀▀▀▀▀▀██ AI REVIEW    ███▀▀▀▀███ DESIGN
 ▀    ▀ ▀▀    ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀ ▀▀  ██▀▀▀▀▀▀██ 3 PRs        ███▀██▀███ 2 PRs        ███▀▀▀▀███ 2 PRs
 1 BEGGING 7   2 MY PRS 3  ●1 new            █▀▀▀▀▀▀▀▀█ just approve ██▀█▀▀█▀██ bot reviews  ██████████ humans think
 updated just now · org:Autofleet            ███▀██▀███              █▀██▀▀██▀█              █▀▀▀▀▀▀▀▀█
 ▚▞ RUBBER STAMP · 3 ▔▔▔▔ mechanical change, approve as-is                    ▛ api#812 ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▜
▌ STAMP  api#812       Bump serde to 1.0.230 across works…    +14 -14  1d ✓ ○ ▌ █▀▀▀▀▀▀▀▀█ RUBBER STAMP              ▐
  STAMP  infra#90      Pin terraform provider versions        +22 -22  1d ✓ ✓ ▌ ██▀▀▀▀▀▀██ mechanical change, approve▐
  STAMP  web#77        Rename FleetCard props (no behavio…    +48 -48  5h ◐ ○ ▌ ██▀▀▀▀▀▀██ as-is                     ▐
 ▚▞ AI REVIEW · 2 ▔▔▔▔ let an AI agent go over it                             ▌ █▀▀▀▀▀▀▀▀█ by @dana · 1d             ▐
   A.I   dispatch#1290 Add retry budget to the matcher cl…   +212 -40  2d ✓ ○ ▌ ███▀██▀███                           ▐
   A.I   dispatch#1294 Typed config loader for routing se…  +330 -120  8h ✗ ○ ▌                                      ▐
 ▚▞ DESIGN REVIEW · 2 ▔▔▔▔ needs a human design review                        ▌ “lockfile only”                      ▐
  DSIGN  billing#431   Split invoice ledger into events +… +1840 -610  3d ✓ ✗ ▌                                      ▐
  DSIGN  infra#88      Move workers to spot node pools        +96 -12 20h · ○ ▌ Bump serde to 1.0.230 across         ▐
                                                                              ▌ workspace                            ▐
                                                                              ▌ acme/api                             ▐
                                                                              ▌                                      ▐
                                                                              ▌ author   @dana                       ▐
                                                                              ▌ opened   1d ago                      ▐
                                                                              ▌ size     +14 -14                     ▐
                                                                              ▌ ci       ✓ passing                   ▐
                                                                              ▙▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▟
```

`tab` (or `2`) switches to **My PRs**: your own open PRs in the same orgs, with
their declared kind (if any), unresolved/total review threads, review state, CI
and mergeability. PRs where someone commented, reviewed or replied in a thread
since you last looked float to the top under **NEW ACTIVITY** (bots and your
own comments do not count). Opening a PR from this screen, or pressing `x`,
marks it seen.

```text
 ↑↓  move   ⏎  open   c  beg/change kind   f  filter   tab  screen   r  refresh   ?  help   q  quit

 █▀▀▄ █▀▀▄    █▀▀▄ █▀▀▀ ▄▀▀▀ ▄▀▀▀ █▀▀▀ █▀▀▄  █▀▀▀▀▀▀▀▀█              █▀▀▀▀▀▀▀▀█              █▀▀▀██▀▀▀█
 ▀▀▀▀ ▀▀▀▀    ▀▀▀▀ ▀▀▀  ▀ ▀▀ ▀ ▀▀ ▀▀▀  ▀▀▀▀  ██▀▀▀▀▀▀██ RUBBER STAMP ██▀▀▀▀▀▀██ AI REVIEW    ███▀▀▀▀███ DESIGN
 ▀    ▀ ▀▀    ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀ ▀▀  ██▀▀▀▀▀▀██ 1 PR         ███▀██▀███ 0 PRs        ███▀▀▀▀███ 1 PR
 1 BEGGING 7   2 MY PRS 3  ●1 new            █▀▀▀▀▀▀▀▀█ just approve ██▀█▀▀█▀██ bot reviews  ██████████ humans think
 updated just now · org:Autofleet            ███▀██▀███              █▀██▀▀██▀█              █▀▀▀▀▀▀▀▀█
 ▚▞ ● NEW ACTIVITY · 1   new since you last looked · x marks seen             ▛ infra#88 ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▜
▌ DSIGN  infra#88      ● Move workers to spot node po…  ◆2/5 blocked  20h · ○ ▌ █▀▀▀██▀▀▀█ DESIGN REVIEW             ▐
 ▚▞ MY OPEN PRS · 2                                                           ▌ ███▀▀▀▀███ needs a human design      ▐
  STAMP  infra#90        Pin terraform provider versi…  ◇0/1 ready     1d ✓ ✓ ▌ ███▀▀▀▀███ review                    ▐
    ?    dispatch#1301   WIP: zone-aware batching          – draft     3h ◐ · ▌ ██████████ by @you · 19h             ▐
                                                                              ▌ █▀▀▀▀▀▀▀▀█                           ▐
                                                                              ▌                                      ▐
                                                                              ▌ “cost vs. eviction risk tradeoff”    ▐
                                                                              ▌                                      ▐
                                                                              ▌ Move workers to spot node pools      ▐
                                                                              ▌ acme/infra                           ▐
                                                                              ▌                                      ▐
                                                                              ▌ author   @you                        ▐
                                                                              ▌ opened   20h ago                     ▐
                                                                              ▌ size     +96 -12                     ▐
                                                                              ▌ ci       · no checks                 ▐
                                                                              ▌ review   ○ 2 pending                 ▐
```

(These are the real renderer's output for demo data:
`cargo test print_screens -- --ignored --nocapture`, with `W`/`H` env vars for
the size. In a terminal the sprites and chips are in colour.)

### Keys

| key | action |
|---|---|
| `j`/`k`, `↓`/`↑` | move (`g`/`G` top/bottom, `pgup`/`pgdn`, `ctrl+d`/`ctrl+u`) |
| `enter` / `o` | open the PR in your browser |
| `c` / `b` | beg: set or change the CR kind of the selected PR (posts as you) |
| `f` / `F` | filter by kind (cycles all → stamp → ai → design) |
| `tab`, `1`, `2` | begging board / my PRs |
| `x` | mark a PR's activity as seen |
| `r` | refresh now (it also refreshes every `refresh_secs`) |
| `?` | help and the glyph legend |
| `q` / `esc` | quit |

Glyphs: CI `✓` passing, `✗` failing, `◐` running, `·` none. Review `✓`
approved, `✗` changes requested, `○` waiting for reviewers. Threads `◆2/5` =
2 unresolved of 5. Status is shape first and colour second, so it reads in
monochrome too.

### Plain output

```sh
pr-begger list          # the board, one line per PR
pr-begger list --mine   # your PRs
```

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

`--org X` and `--repo owner/name` (both repeatable) replace the file's scope
for one run, e.g. `pr-begger --repo saar-autofleet/pr-begger`.

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
  CI rollup, review decision, mergeability, threads) is then fetched by id in
  small batches, up to 8 in parallel. GitHub resolves those nested fields one
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
