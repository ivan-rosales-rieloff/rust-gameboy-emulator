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
- Save and restore full emulator state to disk using a robust sectioned serialization format.
- Automatically persist battery-backed RAM to `saves/{title}.catrigestate` with debounced disk writes for safer in-game SAVE behavior.
- Use TCP-based Game Boy link emulation for server/client serial transfer testing.

## Key Features

- CPU execution for the Game Boy instruction set.
- PPU rendering of background and sprites to a framebuffer.
- Cartridge banking and battery-backed save RAM support.
- Safe save/load of emulator state with threaded state decoding to avoid large state stack overflow.
- Desktop frontend that loads ROM files, displays output, and supports serialized state persistence.
- TCP link mode for serial port emulation between two running instances.

## App Actions

The frontend app allows you to:

- Load a Game Boy ROM file.
- Run the emulation loop and render frames.
- Save and load full emulator state files.
- Exit cleanly and save battery-backed RAM when supported.
- Use configured gamepad or keyboard input through the frontend inputs.
- Configure TCP serial link mode as server or client for linked emulation.

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
- Save/load full emulator state to disk.
- Use networked serial link emulation over TCP.
- Exit: stop the emulator cleanly.

### Frontend Hotkeys

- `L` — Load a new ROM.
- `S` — Save emulator state to a file.
- `O` — Open and load a saved emulator state file.
- `N` — Open/close the network menu.
- `M` — Cycle network mode (`None`, `Server`, `Client`).
- `H` — Cycle host target (`127.0.0.1`, `localhost`, `0.0.0.0`).
- `Up` / `Down` — Adjust the network port number.
- `C` — Connect or disconnect the serial link.
- `Esc` / close window — quit the emulator.
- `G` — Save current cartridge RAM to chosen .sav file (export save)
- `U` — Load a chosen .sav into cartridge RAM (import save)

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
- Battery-backed save RAM is persisted automatically to `saves/{title}.catrigestate` when supported, with debounced writes to reduce disk I/O.
- Temporary reproduction/test projects such as `tmp_repro` and `tmp_state_repro` have been removed from the repository.
- The `core-gb` crate includes smoke tests that verify rendering non-blank frames for known ROMs.

## Disclaimer

This application was developed with the assistance of an AI code assistant.

## Repository Structure

- `Cargo.toml` - workspace manifest.
- `core-common/` - shared utilities.
- `core-gb/` - Game Boy emulator implementation.
- `core-gba/` - Game Boy Advance emulator implementation.
- `frontend-desktop/` - desktop GUI and runtime launcher.
