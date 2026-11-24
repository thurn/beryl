plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    // Different from the Tauri app's ca.thurn.beryl, so both can be installed.
    namespace = "ca.thurn.beryl"
    compileSdk = 36
    defaultConfig {
        applicationId = "ca.thurn.beryl"
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "0.1"
        ndk {
            abiFilters += "arm64-v8a"
        }
    }
    sourceSets {
        // The folder holding arm64-v8a/libberyl_client.so, which `just client android-smoke` builds.
        getByName("main").jniLibs.srcDirs(findProperty("beryl.jniLibs") ?: "jniLibs")
    }
    buildFeatures {
        buildConfig = true
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    packaging {
        jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

dependencies {
    // FileProvider replaces writing a content provider by hand to share report files.
    implementation("androidx.core:core:1.13.1")
}
