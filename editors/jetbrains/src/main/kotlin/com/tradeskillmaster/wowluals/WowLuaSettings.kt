package com.tradeskillmaster.wowluals

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage

@State(name = "WowLuaLsSettings", storages = [Storage("wowlua-ls.xml")])
class WowLuaSettings : PersistentStateComponent<WowLuaSettings.State> {
    class State {
        var lsp4ijServerDisabled: Boolean = false
    }

    private var state = State()

    /**
     * Server-level disable written by LSP4IJ's own "Language Servers" UI (via
     * `LanguageServerEnablementSupport`): turns the WoW Lua server off without
     * uninstalling the plugin. Negative polarity so a fresh install defaults to
     * enabled.
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
