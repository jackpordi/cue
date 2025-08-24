# Cue - Monorepo Orchestration and Caching Tool

A fast, efficient monorepo orchestration and caching tool written in Rust, similar to NX or Turborepo.

## Features

- **Fast Task Execution**: Parallel task execution with intelligent dependency resolution
- **Smart Caching**: Hybrid Content-Addressable Store (CAS) + SQLite caching for build artifacts
- **Hermetic Builds**: Reproducible builds with content-based caching
- **Rust Performance**: Built in Rust for maximum performance and reliability

## Architecture

The project is structured as a Rust workspace with two main components:

1. **`cli`** - Command-line interface that users run on their machines
2. **`common`** - Shared library containing types, schemas, and utilities

## Caching Architecture

Cue uses a hybrid caching system combining Content-Addressable Storage (CAS) with SQLite for metadata:

### 1️⃣ Storage Layout

**Content-Addressable Store (CAS)**
- Files stored by hash under `.cue/cache/cas/<first-three-chars-of-hash>/<remaining-hash>`
- Deduplicates identical outputs automatically
- Only raw blobs; metadata is kept in SQLite

**SQLite Database** (`.cue/cache/db.sqlite`)
- `actions`: cache key → exit code, stdout/stderr paths, timestamp, LRU info
- `outputs`: action key → blob hash → relative path
- `blobs`: blob hash → reference count, size, last access timestamp

### 2️⃣ Writing to Cache

1. Build action runs → produces files, stdout, stderr, exit code
2. Each output file is hashed and stored in CAS (3-char subfolders)
3. SQLite transaction:
   - Insert action record (cache key, exit code, stdout/stderr manifest paths, timestamp)
   - Insert outputs mapping action → blob hash → path
   - Increment blobs.refcount for each new blob

### 3️⃣ Reading from Cache

1. Compute cache key from inputs, command, environment, and dependencies
2. Lookup action in SQLite:
   - If hit → retrieve exit code, stdout/stderr, list of output blobs
   - For each output:
     - Fetch blob from CAS (local)
     - Write to expected relative path
     - Update LRU info (last_access timestamp)

### 4️⃣ LRU-based Garbage Collection

1. Define cache size limit or max age
2. Query actions table ordered by last_access → oldest first
3. Delete actions until cache is under limit:
   - For each deleted action:
     - For each output blob → decrement blobs.refcount
     - If refcount = 0 → delete blob from CAS
     - Remove action record and outputs mapping from SQLite

### 5️⃣ Benefits

- **Performance**: 3-character folders reduce filesystem overhead
- **Deduplication**: CAS ensures identical files are stored only once
- **Efficiency**: SQLite provides fast lookups, LRU, and reference-count tracking
- **Safe GC**: No blob is deleted while still referenced by another cached action
- **Hermetic builds**: Outputs can always be materialized from CAS → reproducible builds

## Getting Started

### Prerequisites

- Rust 1.70+ (install via [rustup](https://rustup.rs/))

### Building

```bash
# Build all components
cargo build

# Build specific component
cargo build -p cue
cargo build -p cue-common
```

### Running

```bash
# Run a specific task
cargo run -p cue -- run build

# Build the project
cargo run -p cue -- build

# Run tests
cargo run -p cue -- test

# Clean build artifacts
cargo run -p cue -- clean
```

## Development

### Project Structure

```
cue/
├── Cargo.toml              # Workspace configuration
├── common/                 # Shared library
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── cache.rs        # Cache-related types
│       ├── error.rs        # Error types
│       └── schema.rs       # Internal schemas
├── cli/                    # Command-line interface
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs         # CLI entry point
│       ├── commands/       # Command implementations
│       ├── cache/          # Caching implementation
│       │   ├── mod.rs
│       │   ├── cas.rs      # Content-Addressable Store
│       │   ├── database.rs # SQLite operations
│       │   └── gc.rs       # Garbage collection
│       ├── config.rs       # Configuration handling
│       └── executor.rs     # Task execution
└── cue.toml               # Example configuration
```

### Adding New Commands

1. Add the command to `cli/src/main.rs` in the `Commands` enum
2. Create a new module in `cli/src/commands/`
3. Implement the command logic
4. Add the command to the match statement in `main()`

## Configuration

### Workspace Configuration

Create a `cue.workspace.toml` file in your workspace root:

```toml
[workspace]
name = "my-monorepo"
projects.auto_discover = true
projects.exclude = [
    "node_modules/",
    ".git/",
    "target/",
]

[cache]
remote = "https://cache.company.com"
size_limit = "50GB"
eviction_policy = "lru"
```

#### Project Discovery

By default, cue automatically discovers all `cue.toml` files in your workspace. You can configure this behavior:

**Auto-discovery (default):**
```toml
[workspace]
projects.auto_discover = true
projects.exclude = ["node_modules/", ".git/"]
```

**Manual patterns:**
```toml
[workspace]
projects.auto_discover = false
projects.include = [
    "projects/*/cue.toml",
    "apps/*/cue.toml",
]
projects.exclude = ["node_modules/", ".git/"]
```

### Project Configuration

Create a `cue.toml` file in each project directory:

```toml
[project]
name = "my-project"
description = "A cue project"
version = "0.1.0"

[tasks.build]
command = "cargo build"
inputs = ["src/**/*.rs", "Cargo.toml"]
outputs = ["target/"]
dependencies = []
cache = true
description = "Build the project"

[tasks.test]
command = "cargo test"
inputs = ["src/**/*.rs", "tests/**/*.rs"]
outputs = []
dependencies = ["build"]
cache = true
description = "Run tests"
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests
5. Submit a pull request

## License

MIT License - see LICENSE file for details.
