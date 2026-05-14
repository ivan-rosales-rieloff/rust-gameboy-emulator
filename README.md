# GB GBA Emulator

A Rust-based Game Boy / Game Boy Advance emulator project with a desktop frontend.

## Overview

This repository contains a modular emulator written in Rust with separate crates for the core emulation logic and a desktop frontend.

- `core-gb`: Game Boy emulation core with cartridge support, CPU, PPU, memory bus, and ROM loading.
- `core-gba`: Game Boy Advance core (placeholder / extension for GBA features).
- `core-common`: Shared runtime utilities and headless execution abstractions.
- `frontend-desktop`: Desktop UI for loading ROMs, running the emulator, and displaying output.

## Supported Use Cases

- Load and run Game Boy ROMs (`.gb`) through the desktop frontend.
- Boot and render supported games, including Pokemon Red.
- Validate emulation using unit tests and smoke tests in the `core-gb` crate.
- Support both ROM-only and memory bank controller cartridges (including MBC1 and MBC3 with battery-backed RAM).

## Key Features

- CPU execution for the Game Boy instruction set.
- PPU rendering of background and sprites to a framebuffer.
- Cartridge banking and save RAM support.
- OAM DMA handling and VBlank interrupt generation.
- Desktop frontend that loads ROM files and displays the emulator screen.

## App Actions

The frontend app allows you to:

- Load a Game Boy ROM file.
- Run the emulation loop and render frames.
- Exit cleanly and save battery-backed RAM when supported.
- Use configured gamepad or keyboard input through the frontend inputs.

## Build & Run

From the workspace root, build and run the desktop frontend:

```powershell
cargo run --bin frontend-desktop -- ..\PokemonRed.gb
```

Or run the emulator tests for the `core-gb` crate:

```powershell
cargo test -p core-gb
```

## Controls

The desktop frontend connects emulator input and display logic. Typical actions include:

- Load ROM: choose a `.gb` file to emulate.
- Start emulator: launch CPU/PPU execution and render frames.
- Save game: battery-backed cartridges save RAM state when exiting.
- Exit: stop the emulator cleanly.

### Button Definitions

The emulator supports the following Game Boy controls with the desktop frontend key mappings:

- `Up` — `Up Arrow`
- `Down` — `Down Arrow`
- `Left` — `Left Arrow`
- `Right` — `Right Arrow`
- `A` — `Z`
- `B` — `X`
- `Start` — `Enter`
- `Select` — `Space`

## Notes

- The project is still under active development and may not support every Game Boy instruction or cartridge type yet.
- The `core-gb` crate includes smoke tests that verify rendering non-blank frames for known ROMs.

## Repository Structure

- `Cargo.toml` - workspace manifest.
- `core-common/` - shared utilities.
- `core-gb/` - Game Boy emulator implementation.
- `core-gba/` - Game Boy Advance emulator implementation.
- `frontend-desktop/` - desktop GUI and runtime launcher.
