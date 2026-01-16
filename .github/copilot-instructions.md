# Project Overview
This project is a Rust application that bridges **Ableton Link** to **MIDI Clock**.
It connects to an Ableton Link session to retrieve BPM and Phase information and sends MIDI Clock (`0xF8`) messages to a selected MIDI output device.

## Key Features
- **Ableton Link Integration**: Connects to Link session to sync tempo.
- **MIDI Clock Output**: Sends MIDI clock ticks (24 PPQN) to sync external hardware or software.
- **System Tray Support**: The application runs in the background and can be minimized to the system tray.
- **UI**: Built with `egui` for configuration (MIDI device selection, status display).

## Tool Kit
- **Language**: Rust
- **UI Framework**: `egui`, `eframe`
- **MIDI**: `midir`
- **Ableton Link**: `ableton-link-rs` (crate)
- **System Tray**: `tray-icon`
- **Concurrency**: `crossbeam-channel`, `std::thread`

## Architecture
- **Main Thread**: Handles UI rendering and System Tray events.
- **MIDI/Clock Thread**: A high-priority thread that runs the clock loop. It fetches Link state and sends MIDI bytes at precise intervals.
- **Communication**: Channels are used to send commands (e.g., "Change MIDI Device") from UI to the Clock thread, and status updates (e.g., "Current BPM") from Clock thread to UI.

## Code Style
- Clean separation of concerns: `ui.rs`, `midi.rs`, `link.rs`.
- No test classes for now.

## Project TODO (short-term)
A short list of actionable items is maintained in `.github/todo.md` — please check that file for current short-term work items and priorities.
