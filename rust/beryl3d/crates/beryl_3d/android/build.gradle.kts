// The releases the Tauri app's gen/android/build.gradle.kts uses.
buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:9.3.1")
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:2.2.10")
    }
}

allprojects {
    repositories {
        google()
        mavenCentral()
    }
    // Build output goes to the cache folder `just b3d android-smoke` names, not beside the
    // sources.
    findProperty("beryl3d.buildDir")?.let {
        layout.buildDirectory.set(file("$it/${project.name}"))
    }
}
