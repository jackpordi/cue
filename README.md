# Cue - Monorepo Orchestration and Caching Tool

A fast, efficient monorepo orchestration and caching tool written in Rust, similar to NX or Turborepo.

## Features

- **Fast Task Execution**: Parallel task execution with intelligent dependency resolution
- **Smart Caching**: Local and remote caching for build artifacts and task results
- **Cloud Service**: HTTP-based remote cache service for team collaboration
- **Rust Performance**: Built in Rust for maximum performance and reliability

## Architecture

The project is structured as a Rust workspace with three main components:

1. **`cli`** - Command-line interface that users run on their machines
2. **`cloud`** - HTTP service that handles getting and setting cache results
3. **`common`** - Shared library containing types, schemas, and utilities

## Getting Started

### Prerequisites

- Rust 1.70+ (install via [rustup](https://rustup.rs/))

### Building

```bash
# Build all components
cargo build

# Build specific component
cargo build -p cue-cli
cargo build -p cue-cloud
cargo build -p cue-common
```

### Running

#### CLI

```bash
# Run a specific task
cargo run -p cue-cli -- run build

# Build the project
cargo run -p cue-cli -- build

# Run tests
cargo run -p cue-cli -- test

# Clean build artifacts
cargo run -p cue-cli -- clean
```

#### Cloud Service

```bash
# Start the cloud service
cargo run -p cue-cloud

# The service will be available at http://localhost:3000
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
│       └── schema.rs       # API schemas
├── cli/                    # Command-line interface
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs         # CLI entry point
│       ├── commands/       # Command implementations
│       ├── cache.rs        # Cache management
│       ├── config.rs       # Configuration handling
│       └── executor.rs     # Task execution
└── cloud/                  # HTTP service
    ├── Cargo.toml
    └── src/
        ├── main.rs         # Server entry point
        ├── cache.rs        # Cache service
        └── database.rs     # Database operations
```

### Adding New Commands

1. Add the command to `cli/src/main.rs` in the `Commands` enum
2. Create a new module in `cli/src/commands/`
3. Implement the command logic
4. Add the command to the match statement in `main()`

### API Development

The cloud service provides a REST API for cache operations:

- `GET /api/v1/health` - Health check
- `POST /api/v1/cache/get` - Retrieve cache entry
- `POST /api/v1/cache/set` - Store cache entry

## Configuration

Create a `cue.toml` file in your project root:

```toml
[workspace]
name = "my-project"
cache_dir = ".cue/cache"
remote_cache_url = "http://localhost:3000"

[tasks.build]
command = "cargo build"
inputs = ["src/**/*.rs", "Cargo.toml"]
outputs = ["target/"]
dependencies = []
cache = true

[tasks.test]
command = "cargo test"
inputs = ["src/**/*.rs", "tests/**/*.rs"]
outputs = []
dependencies = ["build"]
cache = true
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests
5. Submit a pull request

## License

MIT License - see LICENSE file for details.
