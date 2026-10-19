# Lume IDE

<div align="center">

[![Rust](https://img.shields.io/badge/Rust-2021_Edition-orange?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License: MIT / Apache-2.0](https://img.shields.io/badge/License-MIT%20%2F%20Apache--2.0-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/Tests-48%20passing-brightgreen.svg)]()
[![Startup](https://img.shields.io/badge/Cold%20Start-1.7%20ms-success.svg)]()
[![Memory RSS](https://img.shields.io/badge/Peak%20RSS-3.1%20MB-purple.svg)]()
[![Binary Size](https://img.shields.io/badge/Binary%20Size-7.4%20MB-informational.svg)]()

**An ultra-fast, lightweight terminal IDE (TUI) built in Rust for systems engineers and monorepos.**  
Combines the instant startup of classic terminal editors with the out-of-the-box ergonomics of modern GUI editors.

[Key Features](#key-features) •
[Benchmark](#benchmark-comparison) •
[Architecture](#architecture) •
[Quickstart](#quickstart) •
[Keybindings & Mouse](#keybindings--mouse-navigation) •
[Roadmap](#roadmap)

</div>

---

## Overview

**Lume** is a high-performance terminal IDE designed from scratch in Rust. It delivers the speed and low resource usage expected in server environments and massive monorepos (C, C++, CUDA, Rust, Assembly, Java), while eliminating the steep learning curves, plugin maintenance fatigue, and single-threaded lockups of legacy editors.

Everything in Lume is built for responsiveness: UI rendering runs at a rock-solid 60 FPS (< 16 ms frame budget) and never blocks on file I/O, syntax parsing, or disk flushes.

---

## Key Features

- **Sub-2ms Cold Startup:** Starts in ~1.7 ms (empty buffer) and loads 1,500+ lines of real-world code in ~2.3 ms.
- **Sub-microsecond B-Tree Buffer:** Powered by `ropey`, text insertions and deletions execute in $O(\log N)$ in ~1.5 – 2.1 µs on 100k+ line files, with sub-microsecond line/byte conversions.
- **First-Class Mouse & Trackpad Support:** SGR 1006 mouse protocol out-of-the-box:
  - Single click to place cursor
  - Double click to select word
  - Triple click to select entire line
  - Click-and-drag text selection
  - Smooth vertical & horizontal trackpad / wheel scrolling (`ScrollLeft`, `ScrollRight`)
  - Interactive tabs with click-to-switch and `[x]` close buttons
- **Zero-Flicker TUI:** Full frame synchronization using ANSI Synchronized Updates (`\x1b[?2026h` / `\x1b[?2026l`) and a 128 KB buffered output stream. No screen tearing, no cursor teleporting.
- **Asynchronous Background Syntax Highlighting:** `tree-sitter` incremental AST parsing (C, C++, Rust, Python, Java, JavaScript, TypeScript) and native fast tokenizers (Assembly, Go, JSON, TOML, Markdown, YAML) run on a dedicated worker thread via lock-free `crossbeam-channel` rings.
- **Built-in Tree File Explorer:** Collapsible lateral tree view (`F2` or `Ctrl+B`) with Unicode file icons, directory folding, and hybrid keyboard/mouse navigation.
- **Non-blocking Atomic Autosave & Crash Recovery Journal:**
  - Debounced background autosave (`save_async`) using $O(1)$ buffer snapshots (`Arc<Rope>`), temporary files, `fsync`, and atomic renames.
  - Append-only binary crash recovery journal (`LUMJ\x01`) in `$XDG_CACHE_HOME/lume/journal` guaranteeing recovery even after `kill -9`.
- **Modeless Ergonomics by Default:** Familiar VS Code / standard shortcuts (`Ctrl+S`, `Ctrl+O`, `Ctrl+T`, `Ctrl+W`, `Ctrl+Z`, `Ctrl+Shift+P`) with modal modes planned, plus Which-Key overlay (`F1`).
- **Zero External Runtimes:** Single self-contained binary (~7.4 MB stripped). No Node.js, V8, Python, or Electron runtime dependencies.

---

## Benchmark Comparison

Conducted on Ubuntu 26.04.1 LTS (AMD Ryzen 7 5825U, 16 vCPUs, 14 GiB RAM) using `hyperfine` (warmup iterations + statistical averages), `/usr/bin/time -v`, and Rust `criterion`:

### Startup Latency

| Benchmark Scenario | Lume | Vim 9.1 | Neovim 0.11 | Relative Latency |
| :--- | :---: | :---: | :---: | :---: |
| **Cold Start (Empty Buffer)** | **1.7 ms** | 13.6 ms | 17.2 ms | Vim 7.9x / Neovim 10.0x slower |
| **Real Code (Rust, 1,580 lines)** | **2.3 ms** | 21.3 ms | 32.9 ms | Vim 9.2x / Neovim 14.2x slower |
| **Large File (C, 100k lines / 5.1 MB)** | **25.3 ms** | 46.0 ms | 53.9 ms | Lume 1.8x – 2.1x faster |
| **Massive File (C, 500k lines / 27 MB)** | **119.8 ms** | 155.0 ms | 148.7 ms | Instant interactive load |

> *Internal engine latency:* Initializing Lume's core data structures (`Editor::new()`) takes just **216 nanoseconds** in memory.

### Memory Footprint (Peak Resident Set Size)

| Scenario | Lume | Vim 9.1 | Neovim 0.11 | Notes |
| :--- | :---: | :---: | :---: | :---: |
| **Empty Buffer** | **3.10 MB** | 11.24 MB | 11.19 MB | **~3.6x lower RAM consumption** |
| **Real Code (1,580 lines)** | **3.95 MB** | 11.84 MB | 12.50 MB | **~3.0x lower RAM consumption** |
| **100k Lines (5.1 MB)** | **14.78 MB** | 17.55 MB | 17.82 MB | Highly efficient node packing |
| **500k Lines (27 MB)** | **59.75 MB** | 41.66 MB | 42.10 MB | B-Tree indexing overhead for $O(\log N)$ edits |

### Installation & Dependencies

| Metric | Lume | Vim 9.1 | Neovim 0.11 |
| :--- | :---: | :---: | :---: |
| **Executable Size** | **7.4 MB** *(stripped)* | 4.4 MB | 6.1 MB |
| **Runtime Assets Directory** | **0 MB** *(fully embedded)* | 45.0 MB (`/usr/share/vim`) | 26.0 MB (`/usr/share/nvim`) |
| **Total Disk Footprint** | **7.4 MB** | **49.4 MB** | **32.1 MB** |
| **Dynamic Libraries (`ldd`)** | **4** (`libc`, `libm`, `libgcc`, `ld-linux`) | 13 | 12 (`libuv`, `libmsgpack`, `libluajit`...) |

### Internal Engine Microbenchmarks (Criterion)

Measurements on 100,000 active lines loaded in memory:
- **Character insertion (middle of buffer):** `2.11 µs` (0.002 ms)
- **Character insertion (end of buffer):** `1.53 µs`
- **Character insertion (beginning of buffer):** `1.88 µs`
- **Character deletion (middle of buffer):** `1.74 µs`
- **Line-to-byte conversion (`line_to_byte` at line 50,000):** `99.7 ns`
- **Byte-to-line conversion (`byte_to_line` at line 50,000):** `64.7 ns`

---

## Architecture

Lume is organized as a modular Cargo workspace following strict domain decoupling:

```
lume/
├── Cargo.toml                 # Workspace root configuration
├── benchmark.sh               # Automated benchmarking suite (Lume vs Vim vs Neovim)
├── docs/
│   ├── plan.md                # Architectural roadmap and implementation specs (M0–M4)
│   └── Benchmark.md           # Full performance metrics report
└── crates/
    ├── lume-core/             # Headless core editing engine
    │   ├── src/buffer/        # B-Tree RopeBuffer (ropey), UTF-8 boundaries, metrics
    │   ├── src/cursor/        # Multi-cursor engine, automatic merge & normalization
    │   ├── src/editor.rs      # Multi-tab editor, multi-line navigation, PageUp/Down
    │   ├── src/history/       # Linear undo/redo stack with property-based test proofs
    │   ├── src/autosave.rs    # Debounced background atomic autosave controller
    │   └── src/journal.rs     # Append-only binary crash recovery journal (LUMJ\x01)
    │
    ├── lume-syntax/           # Syntax highlighting engine
    │   ├── src/lib.rs         # Tree-sitter AST queries + fast native lexers
    │   └── src/worker.rs      # Dedicated background thread with crossbeam channels
    │
    └── lume-tui/              # Terminal frontend
        ├── src/main.rs        # Terminal loop, synchronized frame updates, event router
        ├── src/explorer.rs    # Collapsible directory tree file explorer (F2 / Ctrl+B)
        └── src/prompt.rs      # Interactive modal overlays (Open, Save As, Language)
```

---

## Quickstart

### Prerequisites

- [Rust & Cargo](https://rustup.rs/) (1.70+ recommended)
- A modern terminal supporting 24-bit TrueColor and SGR mouse reporting (Alacritty, WezTerm, Kitty, Ghostty, iTerm2, Windows Terminal, GNOME Terminal)

### Build & Run

```bash
# Clone the repository
git clone https://github.com/nxtbhz/lume.git
cd lume

# Build optimized release binary
cargo build --release

# Run Lume
./target/release/lume [optional_file_path]
```

### Running Tests

Lume features an extensive automated test suite covering unit tests, crash recovery simulations, and property-based invariance tests:

```bash
cargo test
```

### Running Benchmarks

```bash
# Run Criterion microbenchmarks for lume-core
cargo bench -p lume-core

# Run automated end-to-end comparison against Vim and Neovim
./benchmark.sh
```

---

## Keybindings & Mouse Navigation

### File & Tab Management
| Shortcut | Action |
| :--- | :--- |
| `Ctrl+T` | Open a new tab |
| `Ctrl+W` | Close the active tab |
| `Ctrl+S` | Save current file |
| `Ctrl+Shift+S` | Save file as... (modal prompt) |
| `Ctrl+O` | Open file (modal prompt with path completion) |
| `Ctrl+B` or `F2` | Toggle File Explorer lateral tree |
| `Ctrl+Tab` | Cycle to next tab |
| `Ctrl+Shift+Tab` | Cycle to previous tab |
| `Ctrl+Q` | Quit editor |

### Navigation & Editing
| Shortcut | Action |
| :--- | :--- |
| `Arrow Keys` | Move cursor up, down, left, right |
| `Shift + Arrows` | Extend text selection |
| `Home` / `End` | Jump to start / end of line (`Shift` extends selection) |
| `PageUp` / `PageDown` | Jump page up / down (`Shift` extends selection) |
| `Ctrl+A` | Select all text in buffer |
| `Ctrl+Z` | Undo last transaction |
| `Ctrl+Y` | Redo last transaction |
| `Ctrl+K M` | Change / override syntax language |
| `Ctrl+Shift+P` | Open Command Palette |
| `F1` | Open Which-Key shortcuts overlay |

### Mouse & Trackpad Gestures
| Mouse Action | Effect |
| :--- | :--- |
| **Left Click** | Place cursor at mouse position |
| **Double Click** | Select word under cursor |
| **Triple Click** | Select entire line |
| **Click & Drag** | Fluid range selection |
| **Scroll Wheel / Trackpad** | Smooth vertical and horizontal scrolling |
| **Click Tab Header** | Switch active tab |
| **Click `[x]` on Tab** | Close specific tab |
| **Click File in Explorer** | Open file or expand/collapse directory |

---

## Roadmap

- [x] **M0: Core Engine & Latency Validation**
  - High-speed B-Tree buffer based on `ropey`
  - Multi-cursor engine with automatic cursor merging
  - Symmetrical undo/redo transactions verified with `proptest`
  - Criterion microbenchmark suite
- [x] **M1: Functional TUI MVP**
  - Modeless text editing with full mouse SGR 1006 support
  - Non-blocking atomic autosave & append-only crash recovery journal
  - Asynchronous background syntax highlighting with Tree-sitter
  - Collapsible file explorer with Unicode icons
  - Screen flicker and cursor teleporting elimination via ANSI Synchronized Updates (`?2026h`)
  - Trackpad smooth scrolling & PageUp/PageDown navigation
- [ ] **M2: IDE Intelligence (Next Phase)**
  - Native LSP client hub with auto-discovery (`clangd`, `rust-analyzer`, `jdtls`)
  - In-memory trigram indexer for sub-200ms searches across massive monorepos
  - DAP (Debug Adapter Protocol) engine with gutter breakpoints
  - Git gutter diffs and inline blame
- [ ] **M3: Systems Engineering & Monorepos**
  - Synchronized Assembly / Disassembly split view mapped via DWARF line tables
  - Inline C/C++ macro expansion and `#ifdef` block visualization
  - Headless remote agent (`lumed`) with SSH bridge
- [ ] **M4: WASM Extensibility**
  - Sandboxed plugin engine powered by `wasmtime` (WASI)
  - Safe extension API for custom commands, syntax grammars, and themes

---

## License

Dual-licensed under either:
- **MIT License** ([LICENSE-MIT](LICENSE-MIT) or [http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))
- **Apache License, Version 2.0** ([LICENSE-APACHE](LICENSE-APACHE) or [http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0))

at your option.
