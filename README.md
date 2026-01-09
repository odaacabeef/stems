# stems

Terminal-based multi-track audio recorder with MIDI clock sync.

## Features

- **Multi-track recording** - One track per input channel (supports 32+ channels)
- **Mix recording** - Record the monitored stereo mix to a single file
- **MIDI clock sync** - Recording starts on first clock pulse after MIDI Start
- **Real-time monitoring** - Low-latency monitoring with configurable routing
- **Aggregate device support** - Route audio from virtual devices to physical interfaces
- **Lock-free audio** - Real-time safe audio callbacks (atomics + ring buffers, no mutexes)
- **Per-track control** - Individual arm/monitor/solo/level/pan for each track

**Note:** stems is a recorder only. It does not provide playback or arrangement
features. Import the recorded WAV files into your DAW or audio editor for mixing
and arrangement.

## Usage

**Installation:** There's no binary distribution so you must compile it. Use
`make build` or `make install`.

```sh
# Command help
stems --help

# List available devices
stems --list

# Single audio device
stems --audio-device ES-9 --midi-device mc-source-b

# Aggregate device with monitor routing
stems --audio-device "BlackHole + ES-9" \
      --monitor-channels 17-18 \
      --midi-device mc-source-b
```

### Command Line Flags

- `--audio-device <name>` - Audio device for both input and output (ensures single clock domain)
- `--monitor-channels <START-END>` - Output channels for monitoring (e.g., `1-2`, `17-18`)
  - Defaults to `1-2` if not specified
  - Must be exactly 2 channels (stereo)
  - Channel numbers are 1-indexed
- `--midi-device <name>` - MIDI device for transport control
- `--list-devices` - Show all available audio and MIDI devices

## Interface

![screenshot](docs/screenshot.png)

### Commands

```
j/k, ↑/↓  = Navigate tracks (including mix recording row)

h/l, ←/→  = Navigate columns (Arm/Monitor/Solo/Level/Pan)

Space     = Toggle arm/monitor or edit level/pan

R         = Toggle arm for all tracks

M         = Toggle monitoring for all tracks

g/G       = Jump to first track / mix recording row

?         = Toggle help

q, ctrl+c = quit
```

## Recording Output

### Individual Track Files
- **Format:** 32-bit float WAV, mono per track
- **Filename:** `{track}-{timestamp}.wav` (e.g., `01-20240115-143022.wav`)
- **Sample rate:** Matches input device sample rate
- **Location:** Current working directory

### Mix File
- **Format:** 32-bit float WAV, stereo
- **Filename:** `mix-{timestamp}.wav`
- **Content:** Recorded stereo mix of all monitored tracks with level and panning applied
- **Arming:** Toggle the mix recording checkbox at the bottom of the track list

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
- Sample rate automatically selected at 48000 Hz if supported by device
