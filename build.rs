// Build script to compile Objective-C CoreAudio wrapper on macOS

fn main() {
    #[cfg(target_os = "macos")]
    {
        // Compile the Objective-C file
        cc::Build::new()
            .file("src/audio/coreaudio_playback_ffi.m")
            .flag("-ObjC")
            .compile("coreaudio_playback_ffi");

        // Link CoreAudio frameworks
        println!("cargo:rustc-link-lib=framework=AudioToolbox");
        println!("cargo:rustc-link-lib=framework=CoreAudio");

        // Tell cargo to rerun if the source files change
        println!("cargo:rerun-if-changed=src/audio/coreaudio_playback_ffi.m");
        println!("cargo:rerun-if-changed=src/audio/coreaudio_playback_ffi.h");
    }
}
