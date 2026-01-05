# stems

Terminal-based multi-track audio recorder with MIDI clock sync.

## Features

- **Multi-track recording** - One track per input channel (supports 32+ channels)
- **MIDI clock sync** - Recording starts on first clock pulse after MIDI Start
- **Real-time monitoring** - Low-latency monitoring with configurable routing
- **Aggregate device support** - Route audio from virtual devices (like BlackHole) to physical interfaces
- **Lock-free audio** - Real-time safe audio callbacks (atomics + ring buffers, no mutexes)
- **Per-track control** - Individual arm/monitor/level/pan for each track

## Quick Start

List available devices:
```sh
cargo run --release -- --list-devices
```

Basic usage:
```sh
# Single audio device
cargo run --release -- --audio-device "ES-9" --midi-device mc-source-b

# Aggregate device with monitor routing
cargo run --release -- --audio-device "BlackHole + ES-9" \
                       --monitor-channels 17-18 \
                       --midi-device mc-source-b
```

## Usage

### Command Line Flags

- `--audio-device <name>` - Audio device for both input and output (ensures single clock domain)
- `--monitor-channels <START-END>` - Output channels for monitoring (e.g., `1-2`, `17-18`)
  - Defaults to `1-2` if not specified
  - Must be exactly 2 channels (stereo)
  - Channel numbers are 1-indexed
- `--midi-device <name>` - MIDI device for transport control
- `--list-devices` - Show all available audio and MIDI devices

### Aggregate Device Setup

To record from software applications while monitoring through a physical interface:

1. **Create aggregate device** in Audio MIDI Setup (macOS):
   - Applications → Utilities → Audio MIDI Setup
   - Click **+** → Create Aggregate Device
   - Check both devices (e.g., BlackHole 16ch + ES-9)
   - Set physical interface as **Clock Source**
   - Name it (e.g., "BlackHole + ES-9")

2. **Configure source application** (e.g., synthesizer):
   - Set output device to BlackHole

3. **Run stems** with aggregate device:
   ```sh
   cargo run --release -- --audio-device "BlackHole + ES-9" \
                          --monitor-channels 17-18 \
                          --midi-device mc-source-b
   ```

**Channel mapping:** For an aggregate with BlackHole (16ch) + ES-9 (16ch):
- Channels 1-16: BlackHole inputs/outputs
- Channels 17-32: ES-9 inputs/outputs

Use `--monitor-channels 17-18` to hear audio through ES-9 outputs.

## Interface

![screenshot](docs/screenshot.png)

### Commands

```
j/k, ↑/↓  = Navigate tracks

h/l, ←/→  = Navigate columns (Arm/Monitor/Level/Pan)

Space     = Toggle arm/monitor or edit level/pan

R         = Toggle arm for all tracks

M         = Toggle monitoring for all tracks

?         = Toggle help

q, ctrl+c = quit
```

## Recording Output

- **Format:** 32-bit float WAV, mono per track
- **Filename:** `{track}-{timestamp}.wav` (e.g., `01-20240115-143022.wav`)
- **Sample rate:** Matches input device sample rate
- **Location:** Current working directory

**Note:** stems is a recorder only. It does not provide playback or arrangement
features. Import the recorded WAV files into your DAW or audio editor for mixing
and arrangement.

## Architecture

- **Lock-free audio callbacks** - Uses atomics and ring buffers (no mutexes in real-time thread)
- **Separate input/output streams** - Independent recording and monitoring paths
- **Single clock domain** - Input and output use the same device (eliminates clock drift)
- **Real-time safe** - Audio callbacks never allocate, block, or do I/O

For detailed architecture information, see [docs/architecture.md](docs/architecture.md).

## Technical Notes

- One track is created for each input channel of the selected device
- Monitoring mixes all monitored tracks into stereo and routes to specified output channels
- MIDI clock-based recording waits for first clock pulse after MIDI Start message
- Supports devices with 32+ channels (e.g., aggregate devices)
- Sample rate automatically selected at 48000 Hz if supported by device
