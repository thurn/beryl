// The Android host of Beryl 3D's phone spike (docs/beryl-3d.md, "Hosts"). `just b3d
// android-smoke` builds it with the Tauri app's Gradle wrapper, so that both use the same
// Gradle, Android Gradle Plugin and Kotlin releases, already in Gradle's cache.
rootProject.name = "beryl3d"
include(":app")
