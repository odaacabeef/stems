/**
 * CoreAudio playback engine - C API
 *
 * Simple C interface for low-latency audio playback using CoreAudio directly.
 * Provides immediate stop control that cpal cannot achieve.
 */

#ifndef COREAUDIO_PLAYBACK_FFI_H
#define COREAUDIO_PLAYBACK_FFI_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

// Opaque handle to the playback engine
typedef struct CAPlaybackEngine CAPlaybackEngine;

/**
 * Callback function to fill audio buffer
 *
 * @param user_data User-provided context pointer
 * @param buffer Output buffer to fill (interleaved float samples)
 * @param num_frames Number of frames to fill
 * @param num_channels Total number of output channels
 */
typedef void (*CAPlaybackCallback)(void* user_data, float* buffer, uint32_t num_frames, uint32_t num_channels);

/**
 * Create a new CoreAudio playback engine
 *
 * @param sample_rate Sample rate in Hz (e.g., 48000)
 * @param num_channels Total number of output channels
 * @param device_id AudioDeviceID to use (0 for default)
 * @param callback Function to call to fill audio buffer
 * @param user_data User context passed to callback
 * @return Opaque handle to engine, or NULL on failure
 */
CAPlaybackEngine* ca_playback_create(
    double sample_rate,
    uint32_t num_channels,
    uint32_t device_id,
    CAPlaybackCallback callback,
    void* user_data
);

/**
 * Start playback
 *
 * @param engine Engine handle
 * @return true on success, false on failure
 */
bool ca_playback_start(CAPlaybackEngine* engine);

/**
 * Destroy the playback engine and free resources
 *
 * @param engine Engine handle
 */
void ca_playback_destroy(CAPlaybackEngine* engine);

/**
 * Find an audio device by name
 *
 * @param device_name Device name to search for
 * @return AudioDeviceID if found, 0 if not found
 */
uint32_t ca_find_device_by_name(const char* device_name);

#ifdef __cplusplus
}
#endif

#endif // COREAUDIO_PLAYBACK_FFI_H
