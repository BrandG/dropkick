# Rust and Cargo, for someone who hasn't used them

Written for an experienced programmer coming from Python. It skips language
basics and concentrates on where Rust and Cargo differ from what you already
know, using this repository as the example throughout.

## The one-sentence version

**Cargo is `pip` + `venv` + `pytest` + `setuptools` + `make`, in one tool.**
There is no separate virtualenv step, no `requirements.txt` vs `setup.py` split,
and no test runner to choose. It ships with the language and everyone uses it
the same way.

## Vocabulary

| Rust term | Closest equivalent |
|---|---|
| **crate** | A Python package — one unit of compilation. Either a *library* or a *binary*. |
| **package** | A directory with a `Cargo.toml`. May produce one library plus several binaries. |
| **workspace** | A monorepo of packages sharing one lockfile and one `target/` directory. |
| **module** (`mod`) | A Python module. Usually one file. |
| **crates.io** | PyPI. |
| **`Cargo.toml`** | `pyproject.toml` — what you *want*. |
| **`Cargo.lock`** | A lockfile — exact resolved versions. Committed, for binaries. |
| **trait** | An ABC, a `Protocol`, an interface. |

"Crate" is overloaded and it is genuinely confusing at first:
`smithay-client-toolkit` is a crate and so is `dropkick-core`. Same concept —
one of them is just ours.

## This repository, annotated

```
Dropkick/
├── Cargo.toml            workspace root: members, shared settings, build profiles
├── Cargo.lock            exact versions of all 61 crates. committed on purpose
├── deny.toml             cargo-deny config: the no-networking enforcement
├── target/               ALL build output. disposable, gitignored, ~538 MB
└── crates/
    ├── dropkick-core/    a LIBRARY crate. pure logic, zero dependencies
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs         library entry point; declares the modules below
    │       ├── key.rs         mod key
    │       ├── app.rs         mod app
    │       ├── calculator.rs  mod calculator
    │       └── notepad.rs     mod notepad
    └── dropkick-linux/   a library PLUS two binaries
        ├── Cargo.toml
        └── src/
            ├── lib.rs           the library part (font, ipc, render)
            ├── main.rs          binary #1: dropkickd
            ├── font.rs, render.rs, ipc.rs
            └── bin/
                └── dropkickctl.rs   binary #2
```

Two conventions here are load-bearing. `src/lib.rs` is *the* library entry
point, and `src/main.rs` is *the* default binary. Any additional file in
`src/bin/` automatically becomes another binary — which is why `dropkickctl.rs`
needed no configuration in order to exist.

`lib.rs` saying `pub mod calculator;` is what makes `calculator.rs` part of the
build. Unlike Python, a `.rs` file sitting in the directory is invisible until
something declares it. Forgetting that line is the classic first-week
confusion.

## Commands

Build everything, optimised:

```
cargo build --release
```

Run a specific binary. The `--` separates cargo's own arguments from the ones
passed to the program:

```
cargo run --release --bin dropkickd -- --shot /tmp/dk
```

A bare `cargo run` fails in this workspace, with a helpful error:

```
error: `cargo run` could not determine which binary to run.
available binaries: dropkickctl, dropkickd
```

| command | what it does |
|---|---|
| `cargo check` | Type-check only, no binary produced. Much faster; use while editing. |
| `cargo test` | Run every test. All 29 finish in under a millisecond — `dropkick-core` does no I/O. |
| `cargo test -p dropkick-core` | Only that crate. |
| `cargo test notepad` | Only tests whose name contains "notepad". |
| `cargo clippy --all-targets` | Lint. Catches real bugs, not only style. |
| `cargo fmt` | Auto-format. Non-negotiable in Rust culture; nobody argues about braces. |
| `cargo tree` | Print the dependency graph. |
| `cargo add <name>` | Add a dependency, editing `Cargo.toml` for you. |
| `cargo clean` | Delete `target/`. It rebuilds. |

## Debug vs release, which matters here

`cargo build` produces a debug build: quick to compile, **slow to run** — often
10–50× slower, with optimisations off and overflow checks on. `cargo build
--release` is the reverse.

For Dropkick this is not academic. Every latency figure in `PROJECT_LOG.md`
came from a release build; a debug build would make the summon timings
meaningless. The workspace `Cargo.toml` configures the release profile
explicitly (`lto`, `codegen-units = 1`, `panic = "abort"`, `strip`), trading
compile time for a faster and smaller binary.

## Where things live

- `target/` — ~538 MB against 132 KB of actual source. It is a cache. Delete it
  whenever; never commit it.
- `~/.cargo/registry/` — ~70 MB of downloaded crate *source*, shared across all
  Rust projects on the machine. Reading it is how you check what a library
  actually does rather than guessing from its documentation.
- No virtualenv. Dependencies are per-project by default, there is no global
  install to pollute, and there is no activation step.

## Reading Rust: six things that appear constantly

**1. `&`, `&mut`, and bare values** — the ownership system. For *reading*, it is
simple:

- `&self` — borrowed read-only
- `&mut self` — borrowed exclusively, may be modified
- `self` or a bare value — ownership is transferred; the caller no longer has it

One owner at a time; when the owner goes out of scope the value is freed. That
is why there is no garbage collector and no `free()`. Writing Rust means arguing
with the compiler about this. Reading it mostly means noticing which functions
can mutate what.

**2. `Option<T>`** — either `Some(value)` or `None`. Python's `None`, except the
compiler forces the empty case to be handled. `self.layer: Option<LayerSurface>`
in `main.rs` means "there may or may not be a layer surface right now", which is
why hiding the panel is literally `self.layer = None`.

**3. `Result<T, E>`** — either `Ok(value)` or `Err(problem)`. Exceptions, but
returned as an ordinary value. The `?` suffix means "if this is an error, return
it from this function immediately" — the equivalent of letting an exception
propagate.

**4. `match`** — a switch the compiler checks for exhaustiveness. Add a fifth
tool to the `ToolId` enum and every `match` over it fails to compile until
handled. For a project with a keyboard grammar to keep consistent, this is the
single most valuable thing Rust offers.

**5. `impl` and `trait`.** `impl Calculator { ... }` is "here are Calculator's
methods" — a Python class body, split out from the data definition.
`impl KeyboardHandler for Dropkick` is "Dropkick satisfies this interface",
which is how the toolkit calls back into our code.

**6. `#[derive(Debug)]`, `#[cfg(test)]`** — attributes, roughly decorators.
`#[derive(...)]` generates boilerplate. `#[cfg(test)]` means "compile this only
when testing".

## What surprises people arriving from Python

**Tests live in the same file as the code.** See the bottom of
`crates/dropkick-core/src/calculator.rs`: the `mod tests` block sits directly
under the evaluator. This is idiomatic rather than lazy, and it lets tests reach
private internals. A separate `tests/` directory exists only for integration
tests.

**Compile times are slow enough to feel.** A clean build here is ~12 seconds,
incremental ones ~3.6 s. Use `cargo check` while iterating.

**The compiler is instructive.** Errors usually name the fix and the exact line.
Read them in full instead of skimming — a real habit shift from Python
tracebacks.

**Versions are caret ranges.** `fontdue = "0.9"` means `>=0.9.0, <0.10.0`.
`Cargo.lock` pins what was actually resolved.

## Suggested reading order

1. `crates/dropkick-core/src/key.rs` — 60 lines, data definitions only
2. `crates/dropkick-core/src/calculator.rs` — a parser and its tests, no exotica
3. `crates/dropkick-core/src/app.rs` — tab switching and the Esc ladder; the
   design decisions in code form
4. `crates/dropkick-linux/src/render.rs` — drawing, straightforward arithmetic
5. `crates/dropkick-linux/src/main.rs` — hardest, being mostly Wayland callbacks
   and toolkit traits

`dropkick-core` is the safe place to experiment: no Wayland, no daemon, no I/O.
Break something, run `cargo test -p dropkick-core`, get an answer in
milliseconds.
