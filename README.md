# pagers

A modern alternative to [vmtouch](https://hoytech.com/vmtouch/). Query, touch, evict, and lock the Linux page cache — with a live TUI, `cachestat(2)` support, and no dependencies beyond a Rust toolchain.

## Install

```
cargo install --path crates/pagers-cli
```

Or build from source:

```
cargo build --release
```

The binary lands at `target/release/pagers`.

## Usage

```
# How much of /var/db is cached?
pagers query /var/db

# Load a database into cache before starting your app
pagers touch /var/lib/mysql/ibdata1

# Evict log files to free memory for something else
pagers evict /var/log

# Lock critical files in RAM (won't be paged out)
pagers lock -d /var/lib/redis/dump.rdb

# Machine-readable output for scripts
pagers query -o json /data
pagers query -o kv /data
```

## Subcommands

| Command   | What it does |
|-----------|-------------|
| `query`   | Show which pages of a file are in RAM |
| `touch`   | Load pages into the page cache |
| `evict`   | Drop pages from the page cache |
| `lock`    | Touch + mlock pages in physical memory |
| `lockall` | Lock + mlockall(MCL_CURRENT) |

## Common options

```
-f            Follow symbolic links
-F            Stay on same filesystem
-m 100M       Skip files larger than 100M
-p 0..1G      Only operate on first 1G of each file
-i '*.log'    Ignore files matching pattern
-I '*.db'     Only process files matching pattern
-b paths.txt  Read file paths from a file (- for stdin)
-0            NUL-delimited paths (for use with find -print0)
-o kv|json    Machine-readable output
-q            Quiet (no output)
```

## How it works

`touch` kicks off async readahead with `posix_fadvise(SEQUENTIAL + WILLNEED)`, then walks every page to guarantee residency. Pages the kernel already loaded are instant cache hits; the rest trigger demand faults.

`query` uses `cachestat(2)` on kernel 6.5+ for fast cache queries, falling back to `mincore(2)` on older kernels.

`evict` uses `posix_fadvise(DONTNEED)` on Linux and `msync(MS_INVALIDATE)` on macOS.

When stdout is a terminal, commands show a live TUI with per-file residency maps. When piped, they print a plain-text summary.

## See also

- [vmtouch](https://hoytech.com/vmtouch/) — the original page cache control tool (C, portable)
- [cachestat(2)](https://man7.org/linux/man-pages/man2/cachestat.2.html) — Linux 6.5+ syscall for page cache stats
