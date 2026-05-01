plugins {
    id("java")
    id("org.jetbrains.kotlin.jvm") version "2.1.20"
    id("org.jetbrains.intellij.platform") version "2.11.0"
}

group = "com.tradeskillmaster"
version = "0.0.1"

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    intellijPlatform {
        intellijIdeaUltimate("2025.2")
        bundledPlugin("org.jetbrains.plugins.textmate")
        pluginVerifier()
    }
}

intellijPlatform {
    pluginConfiguration {
        ideaVersion {
            sinceBuild = "252"
            untilBuild = "261.*"
        }
    }
}

tasks {
    prepareSandbox {
        // Lua TextMate grammar copied from editors/vscode/syntaxes/lua.tmLanguage.json.
        // Keep in sync when the VS Code extension grammar is updated.
        from("textmate") {
            into("${intellijPlatform.projectName.get()}/textmate")
        }
    }
}

kotlin {
    jvmToolchain(21)
}
