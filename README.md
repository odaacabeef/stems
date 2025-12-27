# stems

Terminal-based multi-track audio recorder with MIDI clock sync.

## Usage

List available devices:
```sh
cargo run --release -- --list-devices
```

Specify audio and MIDI devices:
```sh
# By index
cargo run --release -- --audio-device 0 --midi-device 1

# By name
cargo run --release -- --audio-device 'BlackHole 16ch' --midi-device 'beefdown-sync'
```

## Interface

![screenshot](/docs/screenshot.png)

## Commands

```
j/k, ↑/↓  = Navigate tracks

h/l, ←/→  = Navigate columns (Arm/Monitor/Level/Pan)

Space     = Toggle arm/monitor or edit level/pan

A         = Toggle arm for all tracks

M         = Toggle monitoring for all tracks

?         = Toggle help

q, ctrl+c = quit
```

## Notes

- Monitoring output always uses system default device (no --output-device flag yet)
- Sample rate mismatch between input/output can cause choppy monitoring audio
- There should be 1 track for every input of your selected audio input device
- Tracks save as 01-YYYYMMDD-HHMMSS.wav (32-bit float, mono)
- Lock-free audio: atomics + ring buffers (no mutex in audio callback)
- Separate input/output streams for recording and monitoring
- MIDI clock-based recording (waits for first clock pulse after Start)
