# MERust

**A from-scratch Rust re-implementation of Mirror's Edge (2009)**: the movement, first-person body, combat and enemy AI, built with [Bevy](https://bevyengine.org/).

It's a port, not a remake. The logic follows the original game's code, scripts and config values as closely as possible, so it should feel like the original rather than "inspired by" it. Faith's moves, the animations, the cops and the guns all behave the way they do in the original game.

> **You need your own legal copy of Mirror's Edge.** This repository contains **no game files**. All models, animations, textures, sounds and settings are loaded at runtime from your own installation. See the [Disclaimer](#disclaimer).

[![Buy Me a Coffee](https://img.shields.io/badge/Buy%20me%20a%20coffee-support-FFDD00?logo=buymeacoffee&logoColor=black)](https://buymeacoffee.com/fy.poolparty)
[![Discord](https://img.shields.io/badge/Discord-join-5865F2?logo=discord&logoColor=white)](https://discord.gg/U4hEjurbxd)

---

## Contents

- [Requirements](#requirements)
- [Getting started](#getting-started)
- [Controls](#controls)
- [Settings and command line](#settings-and-command-line)
- [Project layout](#project-layout)
- [Progress checklist](#progress-checklist)
- [Contributing](#contributing)
- [Support the project](#support-the-project)
- [Credits](#credits)
- [Disclaimer](#disclaimer)
- [License](#license)

---

## Requirements

- **Mirror's Edge (PC)**, legally owned (Steam, EA app / Origin or GOG).
- **Windows 10/11** (the only platform tested so far).
- **Rust**, latest stable, from [rustup.rs](https://rustup.rs/).
- **umodel (UE Viewer)** by Gildor is **included** in `third_party/umodel` (MIT license). It is used once, to export the character, gun and UI meshes from your install into a local `cache/` folder.
- A GPU with Vulkan or DirectX 12 support.

## Getting started

1. Install Mirror's Edge.
2. Clone this repository:
   ```bash
   git clone https://github.com/fy-poolparty/MERust.git
   cd MERust
   ```
3. Build and run. The first build takes a while.
   ```bash
   cargo run -p game --release
   ```
4. On first launch, a folder picker asks for your **Mirror's Edge folder**: the one that contains `TdGame` and `Binaries`, e.g. `C:\Program Files (x86)\Steam\steamapps\common\Mirrors Edge`.
   - The game remembers your choice.
   - You can change it later with **Change folder** in the menu.
5. Pick **Test map** and play.

The first time you start a map, the game exports the meshes it needs with umodel into `cache/`, which takes a moment. Later launches reuse them.

## Controls

| Key | Action |
| --- | --- |
| **W A S D** | Move |
| **Space** | Jump, wallrun, wallclimb, vault, grab |
| **Left Shift** | Crouch / slide |
| **Q** | 180° turn |
| **Left Ctrl** | Walk |
| **Left mouse** | Punch / kick / fire |
| **Right mouse** | Disarm / drop gun / pick up gun |
| **R** | Respawn |
| **N / P** | Next / previous test-map lane |
| **, / .** | Choose an enemy type |
| **B** | Spawn the chosen enemy in front of you |
| **Backspace** | Remove all enemies |
| **V** | Take the chosen enemy's gun |
| **G** | God mode |
| **[ / ]** | Mouse sensitivity down / up |
| **F10** | Back to the menu |
| **Click / Esc** | Capture / release the mouse |

## Settings and command line

The menu has mouse sensitivity, field of view and volume. They are saved to `settings.txt` next to the executable.

Your Mirror's Edge folder is remembered per user in:
- Windows: `%APPDATA%\mirrors-edge-rust\install.txt`
- Linux / macOS: `~/.config/mirrors-edge-rust/install.txt`

Command-line options:

| Option | Effect |
| --- | --- |
| `--map test` | Start the test map directly, skipping the menu |
| `--map tutorial` | Load the real Tutorial map (work in progress) |
| `--me-dir <path>` | Use this Mirror's Edge folder |
| `--god` | Start in god mode |

You can also set the environment variable `MIRRORS_EDGE_DIR` to your Mirror's Edge folder.

## Project layout

| Crate | What it does |
| --- | --- |
| `crates/upk` | Reader for Unreal Engine 3 packages (`.upk`): meshes, animations, physics assets, sounds, textures, properties |
| `crates/tdsim` | The game logic, in Unreal units: physics, every move, weapons, enemy AI, combat |
| `crates/me_level` | Loads everything from your install: levels, props, animations, the animation tree, ragdolls, materials, sounds |
| `crates/game` | The Bevy front end: rendering, input, audio, menu |
| `third_party/umodel` | UE Viewer (umodel) by Gildor, MIT license, used to export meshes from your install |

Folders that are **git-ignored and must never be committed**, because they hold files derived from the game or third-party tools:
- `cache/`: meshes exported by umodel
- `tools/`: local research tools
- `reference/`: research material
- `target/`: build output

---

## Progress checklist

✅ = done, ☐ = still to do

### Movement
- ✅ **Ground:** walking and sprinting with the original acceleration and speed curve, strafing with leg rotation, turning on the spot.
- ✅ **Jumping:** jump, falling, landings (hard, soft, skill roll, fall death), uncontrolled falls, lying on the ground and the backwards get-up roll.
- ✅ **Wall moves:**
  - wallrun, wallrun jump, wallrun dodge;
  - wallclimb, 180 turn-and-kick, wallclimb dodges.
- ✅ **Ledges:** grab, shimmy and corners, turn, pull-up, grab jump, transfers between ledges.
- ✅ **Over obstacles:** vault over and onto (all types), automatic step-up, springboard, coil.
- ✅ **Crouch / slide:** crouch, slide, dodge jump, 180 turn on the ground and in the air.
- ✅ **Climbing:** ladders and pipes, including ladder ↔ pipe transfers.
- ✅ **Traversal:** swing bars, zipline (with the crouch drop), balancing on pipes and beams.
- ✅ **Edge detection:** vertigo and the crouch edge stop.
- ☐ **Remaining player moves:**
  - hand-plant step-up;
  - ledge walk;
  - interact (buttons, valves);
  - barge and door kick;
  - air barge, melee vault, melee barge, hard stumble, rump slide.
- ☐ **Barbed-wire** stumble.
- ☐ **Taunt** during the 180 in the air.

### Body, camera and look
- ✅ **First-person body:** Faith's 1P body driven by the game's own animation tree; camera at the eye joint with the original camera animations.
- ✅ **Camera effects:** swan neck, per-move look limits, camera collision.
- ✅ **Arms and legs:** hands on walls, aim offsets, leg twist, arm reach, arms drawn in their own pass.
- ✅ **Shadow:** a full third-person shadow body that also holds your gun.
- ☐ **Arms' own field of view** (the game's 1P model FOV).
- ☐ **Camera roll and torso lag springs.**
- ☐ **Forearm twist fix** (morph targets).
- ☐ **Hand placement on ledges** and feet on uneven ground.
- ☐ **Materials:** normal and specular maps, skin and cloth shaders.

### Sound
- ✅ Footsteps by surface, sounds triggered by animations, the full sound-node system (velocity and ADSR curves), looping sounds, melee impacts, gun sounds.
- ✅ Volume setting.
- ☐ Wind rush at speed, rooftop ambience, music.

### Combat: player
- ✅ **Melee:** left / right / shove punch combo, jump kick, slide kick with the soccer-kick follow-up, wallrun kick, crouch kick, landing on an enemy from above.
- ✅ **Disarms:** front window, from behind, misses. The gun only becomes usable after the animation and equip time.
- ✅ **Guns:** all 11, with burst, automatic, semi-automatic and shotgun fire, recoil, muzzle flash and crosshair.
- ✅ **Gun rules:** heavy guns limit your moves; pickups, dropping and throwing; per-gun grip in the hands.
- ✅ **Dev tools:** god mode, enemy spawner.
- ☐ **Sniper scope** (M95 zoom).
- ☐ **Unused guns:** DE05, Taser.
- ☐ **Grenades:** flashbang, smoke.
- ☐ **Reaction Time** (slow motion).

### Combat: enemies
- ✅ **Enemy types:** patrol cop, riot cop, SWAT, support and sniper, each with their own body, textures and eyes.
- ✅ **Stats from the game files:** per-class armor, melee blocking, death animations with per-class overrides.
- ✅ **Behaviour:** standing and firing in aimed bursts, melee swings, turning (legs step, body tracks), line of sight, stumbles, fall deaths.
- ✅ **Ragdolls:** the game's own physics assets, with motors, damping and friction tuned to match footage of the original.
- ☐ **Movement AI:** pathing and advancing, running, cover, crouching, jumping, dodging.
- ☐ **More melee:** second swings and counter-attacks.
- ☐ **More enemies:** pursuit cops, Celeste, riot shields, the helicopter.
- ☐ **Physical hit reactions** on living enemies.
- ☐ **Enemy cover aim offsets** and per-gun grip in third person.

### Levels and game
- ✅ **Test map** with lanes A–K covering every obstacle type, plus a combat arena, a ledge and a tower.
- ✅ **Startup menu:** map choice, mouse sensitivity, FOV and volume, saved to a file; asks for and remembers your Mirror's Edge folder.
- 🚧 **Tutorial map:** the real Tutorial_p loads with its materials, ladders and props. It is work in progress and disabled in the menu.
- ☐ **Campaign maps:** loading, level streaming, scripted triggers, doors, elevators, moving platforms.
- ☐ **Time trials:** checkpoints and timing.
- ☐ **HUD:** health vignette, runner-vision effects.

---

## Contributing

Issues and pull requests are welcome. Please:

- **Never commit game files**, or anything extracted or converted from them: meshes, textures, sounds, packages, decompiled scripts, screenshots of extracted assets. Everything has to be loaded from the user's own install at runtime.
- **Port, don't tune.** Behaviour should come from the original game's logic and config values, not from hand-picked numbers.
- Run `cargo test --release --workspace` before opening a pull request. Tests that need the game files read your install from `MIRRORS_EDGE_DIR` or the folder you picked in the menu.

## Support the project

If you like the project and want to support its development:

☕ **[buymeacoffee.com/fy.poolparty](https://buymeacoffee.com/fy.poolparty)**

Join the community on **[Discord](https://discord.gg/U4hEjurbxd)**, and follow **[fy-poolparty on GitHub](https://github.com/fy-poolparty)**.

Donations support the time spent on this free, open-source code. They do **not** buy the game, any game content or any kind of access to it.

## Credits

- **fy_poolparty**: author.
- **DICE** and **Electronic Arts**: Mirror's Edge, the game this project re-implements.
- [Bevy](https://bevyengine.org/): game engine.
- [UE Viewer / umodel](https://www.gildor.org/en/projects/umodel) by Konstantin Nosov (Gildor): mesh export.
- [rfd](https://github.com/PolyMeilex/rfd): native folder picker.
- The Unreal Engine 3 and Mirror's Edge modding communities, for their file-format research.

---

## Disclaimer

**MERust is an unofficial, non-commercial fan project.** It is not affiliated with, endorsed, sponsored or approved by Electronic Arts Inc., EA Digital Illusions CE AB (DICE) or Epic Games.

- **Mirror's Edge**, its characters, names, artwork, models, animations, sounds, music, levels and all related assets are © **EA Digital Illusions CE AB / Electronic Arts Inc.** "Mirror's Edge", "EA" and "DICE" are trademarks of Electronic Arts Inc. "Unreal" is a trademark of Epic Games, Inc. All other trademarks belong to their respective owners.
- **This repository contains no copyrighted game content.** It does not include, and will never distribute, any of the game's files, assets, executables or code. It is original source code that reads data from a copy of the game the user already owns.
- **You must own a legal copy of Mirror's Edge to use this project.** Buy it from an official store such as Steam, the EA app or GOG. This project does not support piracy in any form; please don't ask for, link to or share game files in issues, pull requests or the Discord.
- The project exists for **educational, research, preservation and interoperability purposes**.
- Assets you export with umodel from your own copy stay on your own computer (`cache/` is git-ignored). Don't redistribute them.
- **Rights holders:** if you have any concern about this project, please open an issue or contact the author through the links above, and it will be addressed promptly, up to and including taking the repository down.

This software is provided **"as is", without warranty of any kind**. Use it at your own risk.

## License

The **source code** in this repository is licensed under **MIT OR Apache-2.0**, at your option, as declared in `Cargo.toml`.

This license covers **only the original code written for this project**. It grants **no rights** to Mirror's Edge or any of its content, which remain the property of their respective owners.
