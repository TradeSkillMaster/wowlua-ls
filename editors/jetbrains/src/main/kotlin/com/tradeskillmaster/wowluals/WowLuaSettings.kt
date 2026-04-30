package com.tradeskillmaster.wowluals

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage

@State(name = "WowLuaLsSettings", storages = [Storage("wowlua-ls.xml")])
class WowLuaSettings : PersistentStateComponent<WowLuaSettings.State> {
    class State {
        var serverPath: String = ""
    }

    private var state = State()

    var serverPath: String
        get() = state.serverPath
        set(value) { state.serverPath = value }

    override fun getState(): State = state

    override fun loadState(state: State) {
        this.state = state
    }

    companion object {
        fun getInstance(): WowLuaSettings =
            ApplicationManager.getApplication().getService(WowLuaSettings::class.java)
    }
}
