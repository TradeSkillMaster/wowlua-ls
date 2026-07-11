package com.tradeskillmaster.wowluals

/**
 * Decides which LSP backend serves this session. The built-in (native) client
 * is the default; the LSP4IJ backend is opt-in via settings, and the automatic
 * fallback on IDEs without the built-in LSP module (Community-based IDEs).
 *
 * Both dependencies are optional, so availability is probed reflectively —
 * this class is loaded unconditionally and must not reference either API
 * directly. With an optional plugin dependency present, its classes are
 * visible through this plugin's classloader, so Class.forName reflects
 * "installed AND enabled".
 */
object WowLuaBackend {
    val lsp4ijAvailable: Boolean by lazy {
        classExists("com.redhat.devtools.lsp4ij.LanguageServerFactory")
    }

    val nativeLspAvailable: Boolean by lazy {
        classExists("com.intellij.platform.lsp.api.LspServerSupportProvider")
    }

    /** True when the LSP4IJ backend should run (and the native one stand down). */
    fun useLsp4ij(): Boolean =
        lsp4ijAvailable && (WowLuaSettings.getInstance().useLsp4ij || !nativeLspAvailable)

    private fun classExists(name: String): Boolean = try {
        Class.forName(name, false, javaClass.classLoader)
        true
    } catch (_: ClassNotFoundException) {
        false
    }
}
