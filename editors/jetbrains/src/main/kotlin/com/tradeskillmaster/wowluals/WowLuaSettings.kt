package com.tradeskillmaster.wowluals

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage

@State(name = "WowLuaLsSettings", storages = [Storage("wowlua-ls.xml")])
class WowLuaSettings : PersistentStateComponent<WowLuaSettings.State> {
    class State {
        var useLsp4ij: Boolean = false
        var lsp4ijServerDisabled: Boolean = false
    }

    private var state = State()

    /** Backend selection: run the LSP4IJ backend instead of the built-in client. */
    var useLsp4ij: Boolean
        get() = state.useLsp4ij
        set(value) { state.useLsp4ij = value }

    /**
     * Server-level disable written by LSP4IJ's own "Language Servers" UI (via
     * `LanguageServerEnablementSupport`). Independent of [useLsp4ij]: this turns
     * the LSP4IJ server off without switching backends. Negative polarity so a
     * fresh install defaults to enabled.
     */
    var lsp4ijServerDisabled: Boolean
        get() = state.lsp4ijServerDisabled
        set(value) { state.lsp4ijServerDisabled = value }

    override fun getState(): State = state

    override fun loadState(state: State) {
        this.state = state
    }

    companion object {
        fun getInstance(): WowLuaSettings =
            ApplicationManager.getApplication().getService(WowLuaSettings::class.java)
    }
}
