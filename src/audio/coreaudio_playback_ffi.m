/**
 * CoreAudio playback engine - Objective-C implementation
 *
 * Uses CoreAudio AudioUnit API directly for low-latency playback with immediate stop.
 */

#import <AudioToolbox/AudioToolbox.h>
#import <CoreAudio/CoreAudio.h>
#include "coreaudio_playback_ffi.h"

struct CAPlaybackEngine {
    AudioUnit audio_unit;
    CAPlaybackCallback callback;
    void* user_data;
    uint32_t num_channels;
};

// CoreAudio render callback - called from real-time audio thread
static OSStatus render_callback(
    void* inRefCon,
    AudioUnitRenderActionFlags* ioActionFlags,
    const AudioTimeStamp* inTimeStamp,
    UInt32 inBusNumber,
    UInt32 inNumberFrames,
    AudioBufferList* ioData
) {
    (void)ioActionFlags;
    (void)inTimeStamp;
    (void)inBusNumber;

    CAPlaybackEngine* engine = (CAPlaybackEngine*)inRefCon;

    if (!engine->callback) {
        // No callback - fill with silence
        for (UInt32 i = 0; i < ioData->mNumberBuffers; i++) {
            memset(ioData->mBuffers[i].mData, 0, ioData->mBuffers[i].mDataByteSize);
        }
        return noErr;
    }

    // Get the output buffer (interleaved)
    float* buffer = (float*)ioData->mBuffers[0].mData;

    // Call Rust callback to fill the buffer (Rust controls silence vs audio)
    engine->callback(engine->user_data, buffer, inNumberFrames, engine->num_channels);

    return noErr;
}

CAPlaybackEngine* ca_playback_create(
    double sample_rate,
    uint32_t num_channels,
    uint32_t device_id,
    CAPlaybackCallback callback,
    void* user_data
) {
    CAPlaybackEngine* engine = calloc(1, sizeof(CAPlaybackEngine));
    if (!engine) {
        return NULL;
    }

    engine->callback = callback;
    engine->user_data = user_data;
    engine->num_channels = num_channels;

    // Create an AudioComponentDescription for HAL output (to select specific device)
    AudioComponentDescription desc = {
        .componentType = kAudioUnitType_Output,
        .componentSubType = kAudioUnitSubType_HALOutput,  // Use HAL output to select device
        .componentManufacturer = kAudioUnitManufacturer_Apple,
        .componentFlags = 0,
        .componentFlagsMask = 0
    };

    // Find the component
    AudioComponent component = AudioComponentFindNext(NULL, &desc);
    if (!component) {
        fprintf(stderr, "Failed to find HAL output component\n");
        free(engine);
        return NULL;
    }

    // Create an instance of the audio unit
    OSStatus status = AudioComponentInstanceNew(component, &engine->audio_unit);
    if (status != noErr) {
        fprintf(stderr, "Failed to create audio unit instance: %d\n", status);
        free(engine);
        return NULL;
    }

    // Set the device if specified
    if (device_id != 0) {
        status = AudioUnitSetProperty(
            engine->audio_unit,
            kAudioOutputUnitProperty_CurrentDevice,
            kAudioUnitScope_Global,
            0,
            &device_id,
            sizeof(device_id)
        );

        if (status != noErr) {
            fprintf(stderr, "Failed to set audio device: %d\n", status);
            AudioComponentInstanceDispose(engine->audio_unit);
            free(engine);
            return NULL;
        }
    }

    // Set the stream format
    AudioStreamBasicDescription stream_format = {
        .mSampleRate = sample_rate,
        .mFormatID = kAudioFormatLinearPCM,
        .mFormatFlags = kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
        .mBytesPerPacket = num_channels * sizeof(float),
        .mFramesPerPacket = 1,
        .mBytesPerFrame = num_channels * sizeof(float),
        .mChannelsPerFrame = num_channels,
        .mBitsPerChannel = 32
    };

    status = AudioUnitSetProperty(
        engine->audio_unit,
        kAudioUnitProperty_StreamFormat,
        kAudioUnitScope_Input,
        0,
        &stream_format,
        sizeof(stream_format)
    );

    if (status != noErr) {
        AudioComponentInstanceDispose(engine->audio_unit);
        free(engine);
        return NULL;
    }

    // Set the render callback
    AURenderCallbackStruct callback_struct = {
        .inputProc = render_callback,
        .inputProcRefCon = engine
    };

    status = AudioUnitSetProperty(
        engine->audio_unit,
        kAudioUnitProperty_SetRenderCallback,
        kAudioUnitScope_Input,
        0,
        &callback_struct,
        sizeof(callback_struct)
    );

    if (status != noErr) {
        AudioComponentInstanceDispose(engine->audio_unit);
        free(engine);
        return NULL;
    }

    // Set a small buffer size for low latency (64 frames)
    UInt32 buffer_size = 64;
    status = AudioUnitSetProperty(
        engine->audio_unit,
        kAudioUnitProperty_MaximumFramesPerSlice,
        kAudioUnitScope_Global,
        0,
        &buffer_size,
        sizeof(buffer_size)
    );
    // Continue even if this fails - it's not critical

    // Initialize the audio unit
    status = AudioUnitInitialize(engine->audio_unit);
    if (status != noErr) {
        AudioComponentInstanceDispose(engine->audio_unit);
        free(engine);
        return NULL;
    }

    return engine;
}

bool ca_playback_start(CAPlaybackEngine* engine) {
    if (!engine) {
        return false;
    }

    OSStatus status = AudioOutputUnitStart(engine->audio_unit);
    return status == noErr;
}

void ca_playback_destroy(CAPlaybackEngine* engine) {
    if (!engine) {
        return;
    }

    // Stop the audio unit
    AudioOutputUnitStop(engine->audio_unit);

    // Uninitialize and dispose
    AudioUnitUninitialize(engine->audio_unit);
    AudioComponentInstanceDispose(engine->audio_unit);

    free(engine);
}

uint32_t ca_find_device_by_name(const char* device_name) {
    if (!device_name) {
        return 0;
    }

    // Get the list of all audio devices
    AudioObjectPropertyAddress property_address = {
        .mSelector = kAudioHardwarePropertyDevices,
        .mScope = kAudioObjectPropertyScopeGlobal,
        .mElement = kAudioObjectPropertyElementMain
    };

    UInt32 data_size = 0;
    OSStatus status = AudioObjectGetPropertyDataSize(
        kAudioObjectSystemObject,
        &property_address,
        0,
        NULL,
        &data_size
    );

    if (status != noErr) {
        fprintf(stderr, "Failed to get device list size: %d\n", status);
        return 0;
    }

    UInt32 device_count = data_size / sizeof(AudioDeviceID);
    AudioDeviceID* devices = (AudioDeviceID*)malloc(data_size);
    if (!devices) {
        return 0;
    }

    status = AudioObjectGetPropertyData(
        kAudioObjectSystemObject,
        &property_address,
        0,
        NULL,
        &data_size,
        devices
    );

    if (status != noErr) {
        fprintf(stderr, "Failed to get device list: %d\n", status);
        free(devices);
        return 0;
    }

    // Search for device by name
    AudioDeviceID found_device = 0;
    for (UInt32 i = 0; i < device_count; i++) {
        AudioObjectPropertyAddress name_address = {
            .mSelector = kAudioDevicePropertyDeviceNameCFString,
            .mScope = kAudioObjectPropertyScopeGlobal,
            .mElement = kAudioObjectPropertyElementMain
        };

        CFStringRef cf_name = NULL;
        UInt32 name_size = sizeof(CFStringRef);

        status = AudioObjectGetPropertyData(
            devices[i],
            &name_address,
            0,
            NULL,
            &name_size,
            &cf_name
        );

        if (status == noErr && cf_name) {
            char name_buffer[256];
            if (CFStringGetCString(cf_name, name_buffer, sizeof(name_buffer), kCFStringEncodingUTF8)) {
                if (strcmp(name_buffer, device_name) == 0) {
                    found_device = devices[i];
                    CFRelease(cf_name);
                    break;
                }
            }
            CFRelease(cf_name);
        }
    }

    free(devices);

    if (found_device == 0) {
        fprintf(stderr, "Device '%s' not found\n", device_name);
    }

    return found_device;
}
