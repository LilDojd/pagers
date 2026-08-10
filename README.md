# pagers

[![CI](https://github.com/LilDojd/pagers/actions/workflows/ci.yml/badge.svg)](https://github.com/LilDojd/pagers/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/pagers.svg)](https://crates.io/crates/pagers)
[![codecov](https://codecov.io/gh/LilDojd/pagers/branch/main/graph/badge.svg)](https://codecov.io/gh/LilDojd/pagers)

Portable file system page cache diagnostics and control for Linux and macOS.

`pagers` queries, touches, evicts, and locks the page cache using `cachestat(2)`, `mincore(2)`, `posix_fadvise(2)`, and `mlock(2)`. When stdout is a terminal it renders a live TUI with per-file residency maps. When piped, it emits plain text, key=value pairs, or JSON.

## Install

From [crates.io](https://crates.io/crates/pagers):

```
cargo install pagers
```

From source:

```
cargo install --path crates/pagers-cli
```

With Nix:

```
nix profile install github:LilDojd/pagers
```

..or from [FlakeHub](https://flakehub.com):

```
nix profile install "https://flakehub.com/f/LilDojd/pagers/*"
```

Pre-built binaries for each release are available on the [GitHub releases page](https://github.com/LilDojd/pagers/releases).

### Docker

Container images are published to GHCR and Docker Hub.

```
docker pull ghcr.io/lildojd/pagers:latest
docker run --rm -v /data:/data ghcr.io/lildojd/pagers query /data
```

MSRV: **1.88.0**

## Usage

```sh
# How much of /var/db is cached?
pagers query /var/db

# Load a database file into cache before starting your app
pagers touch /var/lib/mysql/ibdata1

# Evict log files to free memory for active data
pagers evict /var/log

# Lock critical files in RAM so they are never paged out
pagers lock -d /var/lib/redis/dump.rdb

# Lock files and the entire current address space
pagers lockall -d /data

# Machine-readable output
pagers query -o json /data | jq .
pagers query -o kv /data | grep TotalResidentPercent

# Process a list of paths from find(1)
find /srv -name '*.db' -print0 | pagers query -b - -0

# Only consider .dat files under 500M, in the first 1G of each file
pagers query -I '*.dat' -m 500M -p 0..1G /data
```

## Subcommands

| Command | Description |
|-----------|-------------|
| `query` | Show which pages of a file are in the page cache |
| `touch` | Load pages into the page cache |
| `evict` | Drop pages from the page cache |
| `lock` | Touch + `mlock(2)` pages in physical memory |
| `lockall` | Lock + `mlockall(MCL_CURRENT)` |

## Options

All subcommands accept these flags:

```
PATHS              Files or directories to process

-f                 Follow symbolic links
-F                 Stay on the same filesystem
-H                 Count hardlinked copies separately
-m, --max-file-size SIZE
                   Skip files larger than SIZE (e.g. 4k, 100M, 1.5G)
-p, --range RANGE  Byte range to operate on (e.g. 10K-20G, 100M..500M, 0,1G)
-i, --ignore GLOB  Ignore files matching pattern (repeatable)
-I, --filter GLOB  Only process files matching pattern (repeatable)
-b, --batch FILE   Read paths from FILE (- for stdin)
-0                 NUL-delimited paths in batch mode
-j, --threads N    Number of worker threads (0 = all cores)
-o, --output FMT   Output format: human, kv, json (omit on a TTY for the TUI)
-v                 Increase verbosity (repeatable)
-q                 Quiet (no output)
```

`lock` and `lockall` also accept:

```
-d, --daemon       Run as a daemon (block until signal)
--wait             Wait until all pages are locked (requires -d)
-P, --pidfile PATH Write PID to file
```

### Size and range syntax

Sizes accept decimal and binary units: `4k`, `100M`, `1.5G`, `2KiB`, `8MiB`. Scientific notation (`1e2K`) and fractional values (`1.5G`) work too. Ranges can be written as `10K-20G`, `10K..20G`, or `10K,20G`. Open-ended ranges like `-20G` (from start) and `10K-` (to end) are supported.

## How it works

**`query`** uses the `cachestat(2)` syscall on Linux 6.5+ for single-syscall cache stats per file, falling back to `mincore(2)` on older kernels and on macOS. The TUI renders per-file residency maps; CLI modes emit aggregate statistics.

**`touch`** issues separate `posix_fadvise(POSIX_FADV_SEQUENTIAL)` and `posix_fadvise(POSIX_FADV_WILLNEED)` calls on Linux, then walks every missing page with volatile reads to guarantee residency. macOS uses `madvise(MADV_WILLNEED)` before the page walk.

**`evict`** calls `posix_fadvise(POSIX_FADV_DONTNEED)` on Linux and `msync(MS_INVALIDATE)` on macOS to advise the kernel to drop cached pages.

**`lock`** touches pages into cache and then calls `mlock(2)` to wire them into physical memory. **`lockall`** additionally calls `mlockall(MCL_CURRENT)` after locking individual files.

Directories are traversed in parallel using [dua-core](https://crates.io/crates/dua-core), and file operations run on a bounded set of standard-library worker threads. Memory-mapped I/O is handled by [memmap2](https://github.com/RazrFalcon/memmap2-rs). The live TUI uses `ratatui-core` and `ratatui-crossterm`.

## Use cases

- **Database warm-up.** `pagers touch /var/lib/postgresql/data` before starting a service to avoid cold-start I/O.
- **Deployment pre-warming.** Touch shared libraries or application binaries after deploy so the first request does not pay the page-fault penalty.
- **Memory reclamation.** `pagers evict /var/log` to return pages used by rotated logs back to the free pool.
- **Pinning critical data.** `pagers lock -d /var/lib/redis/dump.rdb` keeps a dataset in RAM for the lifetime of the daemon process.
- **Cache auditing.** `pagers query -o json /data` for monitoring scripts that track page cache residency over time.
- **CI/benchmarking.** Evict file caches between benchmark runs for repeatable cold-start measurements.
- **Batch processing.** Pipe paths from `find(1)` or a manifest file via `-b -` to operate on arbitrary file sets.

## Comparison with vmtouch

| | vmtouch | pagers |
|-|---------|--------|
| Language | C | Rust |
| Platforms | Linux, FreeBSD, Solaris, macOS, HP-UX, OpenBSD | Linux, macOS |
| Cache query | `mincore(2)` | `cachestat(2)` on Linux 6.5+, `mincore(2)` fallback |
| Live TUI | Sort of | Yes |
| Daemon mode | `-d` (requires `-l`/`-L`) | `-d` for `lock` and `lockall` |
| Parallel traversal and file processing | No | Yes (`dua-core` + worker threads) |
| Range operations | `-p` page ranges | `-p` byte ranges with unit suffixes |

### Performance

Current measurements use vmtouch 1.3.1 and release builds on Linux 7.1.6.

| Benchmark | vmtouch | pagers | pagers speedup |
|-----------|---------|--------|-----------------|
| Query 10 GiB cached | 45.8 ms | 16.0 ms | **2.87x** |
| Query 10 GiB uncached | 14.8 ms | 0.825 ms | **17.94x** |
| Evict 10 GiB cached | **1.184 s** | 1.257 s | 0.94x |
| Touch 10 GiB uncached | 1.803 s | **1.720 s** | 1.05x |
| Touch 1,000 x 1 MiB uncached files | 393.5 ms | **170.1 ms** | **2.31x** |
| Evict 1,000 x 1 MiB cached files | **135.3 ms** | 186.7 ms | 0.72x |

## See also

- [vmtouch](https://hoytech.com/vmtouch/) — the og page cache control tool
- [cachestat(2)](https://man7.org/linux/man-pages/man2/cachestat.2.html) — Linux 6.5+ syscall for page cache statistics
- [mincore(2)](https://man7.org/linux/man-pages/man2/mincore.2.html) — determine whether pages are resident in memory
- [posix_fadvise(2)](https://man7.org/linux/man-pages/man2/posix_fadvise.2.html) — predeclare an access pattern for file data
- [mlock(2)](https://man7.org/linux/man-pages/man2/mlock.2.html) — lock pages in memory
