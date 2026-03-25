# Changelog

All notable changes to this project will be documented in this file.

## [0.1.5] - 2026-03-25

### Bug Fixes

- Suppress TUI when -q is passed without -o
- Visible_files returns empty list when all files are done and count exceeds max

### Features

- Support q key to quit TUI alongside Ctrl+C

### Performance

- Optimize BitVec::from_residency_bytes with byte-level packing
- *(tui)* Incremental pages_in_core tracking in progress events
- *(tui)* Use Arc<str> for event paths to avoid per-progress string clones

### Refactor

- Fix inconsistencies and code smells across crates
- Replace lazy_static with LazyLock, extract CLI dispatch helpers
- Unify crawl path collection logic
- Replace Cachestat::zeroed with derived Default
- Add TerminalGuard for RAII cleanup of raw mode and cursor
- Implement Widget trait for FileRow and SummaryWidget
- Extract drain_events and deduplicate render logic in main loop
- Introduce FrameContext to fix clippy too_many_arguments warning

## [0.1.4] - 2026-03-25

### Bug Fixes

- *(ci)* Macos-14 -> macos-latest for darwin runners
- Fix tests on linux by flushing

### CI

- *(Mergify)* Configuration update ([#9](https://github.com/LilDojd/pagers/pull/9))

### Documentation

- *(README)* Remove output formats row from README table
- *(README)* Update MSRV and clean up supported targets section

### Features

- *(core)* Add Threads enum for configurable parallelism
- *(core)* Add ThreadPool error variant for rayon pool build failures
- *(core)* Add threads field to CrawlConfig
- *(core)* Use scoped thread pool in crawl_and_process
- *(cli)* Add -j/--threads option for parallel thread control

### Miscellaneous Tasks

- *(ci)* Decrease timeout for flakehub-cache-action
- *(release)* Prepare for v0.1.4

### Performance

- Skip tracing subscriber when no log level is active
- *(core)* Mincore once optimization
- Do not compile debug! and trace! by default

### Refactor

- *(cli)* Use Threads enum directly in CLI via FromStr
- Use NonZeroU16 for Threads::Exact
- Move Threads to par.rs with grouped cfg(rayon) module
- Small ref in par process
- *(cli)* Make Run a generic trait over TuiMode and OutputFormat, add more tracing

## [0.1.3] - 2026-03-24

### Miscellaneous Tasks

- *(release)* Prepare for v0.1.3

## [0.1.2] - 2026-03-24

### Documentation

- *(cliff)* Update cliff.toml

### Miscellaneous Tasks

- *(Cargo.lock)* Update deps
- *(release)* Prepare for v0.1.2

## [0.1.1] - 2026-03-24

### Bug Fixes

- *(ci)* Fix release workflow (hopefully)

### Miscellaneous Tasks

- *(release)* Prepare for v0.1.1

## [0.1.0] - 2026-03-24

### Bug Fixes

- Daemonize
- Linux fixes
- Use BorrowedFd in cachestat
- Wire up summary output for non-TUI mode (piped stdout)
- Flake on macos
- *(macos)* Libc mincore vec pointer type
- Correctly update progress bar by storing page offset in FileProgress
- *(ci)* Proper cross
- *(ci)* Add perms for flakehub cache
- *(core)* Compilation on other targets
- *(core)* Mincore call on different targets
- *(docker)* Try to fix docker build
- *(ci)* Pre-commit build inputs
- *(batch)* Release stdin lock before tui is spawned to avoid race
- *(daemon)* Do not drop the locks vec
- *(ui)* Fix flickering when running with many files
- *(core)* Cachestat returns usize
- *(test)* Linux-specific

### CI

- *(Mergify)* Configuration update ([#8](https://github.com/LilDojd/pagers/pull/8))

### Documentation

- Revise README tone and wording
- Update README.md and set MSRV in Cargo.toml
- *(README)* Update
- Update README with performance comparisons

### Features

- Add dependencies and module skeleton
- Implement pagers
- Add dependabot and ci
- *(core)* Replace Progress trait with event channel in ops
- Add cachestat(2) syscall module with runtime detection
- Use cachestat as fast path in process_file when TUI is off
- Parallelize crawl and processing with rayon
- *(ci)* Change ci-nix to use omnix ci
- *(core)* Feature-gate rayon and dashmap
- *(ui)* Add humantime

### Miscellaneous Tasks

- First commit
- Use clap::value_parser! macro
- Prek
- *(tui)* Review fixes and clippy cleanup
- Use nix for evict
- Update deps
- *(deps)* Bump nixbuild/nix-quick-install-action from 33 to 34 ([#2](https://github.com/LilDojd/pagers/pull/2))
- *(deps)* Bump actions/checkout from 4 to 6 ([#1](https://github.com/LilDojd/pagers/pull/1))
- Remove .pre-commit-config.yaml
- Remove rayon
- Move daemon to a separate module
- Add LockAll op
- Remove sys exits
- Fix clippy warnings and add JSON output test
- Add nextest config with retries for flaky page cache tests
- Switch from Vec<bool> to BitVec for page residency
- Playing around
- Make residency generic over Vec<bool> iterators
- Move ugly stuff from ops/mod.rs to ops/process.rs, will refactor in next commit
- Github actions + crane
- Clippy + treefmt
- Add fenix to flake
- Small nit
- Treefmt
- Propagate bitvec feature and implement faster packed bitvec construction
- Add more tests and rename mmap
- Remove the stupid unreachable! from cached pages
- Prep for release with actions
- Clippy warning
- *(ci)* Improving workflows
- *(nix)* Switch to flakehub inputs and add update lock action
- *(nix)* Try to build containers with nix2container
- *(deps)* Bump mergifyio/gha-mergify-ci from 14 to 16 (#4) ([#4](https://github.com/LilDojd/pagers/pull/4))
- *(deps)* Bump actions/upload-artifact from 4 to 7 (#3) ([#3](https://github.com/LilDojd/pagers/pull/3))
- *(deps)* Bump docker/login-action from 3 to 4 (#6) ([#6](https://github.com/LilDojd/pagers/pull/6))
- *(deps)* Bump actions/checkout from 4 to 6 (#5) ([#5](https://github.com/LilDojd/pagers/pull/5))
- *(deps)* Bump actions/download-artifact from 4 to 8 (#7) ([#7](https://github.com/LilDojd/pagers/pull/7))
- *(dev)* Add gpg to devshell
- Remove x86_64-apple-darwin from supported targets
- *(cli)* Prepare completions with release
- *(ci)* Set timeout for flakehub cache
- Add benchmark script against vmtouch
- Update bench.sh
- *(doc)* Update TUI comment on vmtouch
- *(release)* Prepare for v0.1.0

### Performance

- *(core)* Skip redundant mincore calls in process_file
- *(evict)* Call mincore once in TUI path and 0 times in CLI path
- *(core)* Drop redundant fields from FileDone
- *(ui)* Drain event channel on every frame to avoid FileDone delay

### Refactor

- Reimplement size parsing with SizeRange and delimiter support
- *(tui)* Extract FileState into state.rs
- *(tui)* Extract TuiEvent and thread spawning into event.rs
- *(tui)* Extract rendering into ui.rs with done-file visuals and char-based truncation
- *(tui)* Extract App struct with HashMap index into app.rs
- Wip
- *(ops)* Replace Operation enum with generic Op trait
- Run op
- Replace anyhow with typed errors in pagers-core
- Clean up crawl and pretty_size
- Type output format and mode enums through the stack
- Redesign touch with two-phase fadvise+walk approach
- Remove OutputFormat::Pretty, simplify format plumbing
- Remove Mode enum, use serde_json, make OutputFormat the API
- Move label to Op::LABEL, remove string passing
- DRY up process_file and LockedFile
- Simplify runop, use json! macro, plain bool in TUI
- Stuff
- Move OutputFormat to CLI
- Move stuff around in pagers_core and use lazy_static! where applicable
- Ops and crawl
- Make residency generic with PM type parameter across crates
- *(core)* Refactor the if-else soup that was mode selection into a stronger type system
- *(output)* Refactor into generic trai

### Testing

- Add unit and integration tests

### Other

- Playing around with tui

