package com.tradeskillmaster.wowluals.lsp4ij

import org.eclipse.lsp4j.Hover
import org.eclipse.lsp4j.TextDocumentIdentifier
import org.eclipse.lsp4j.jsonrpc.services.JsonRequest
import org.eclipse.lsp4j.services.LanguageServer
import java.util.concurrent.CompletableFuture

/** The server's requests beyond the LSP ones (see `WowLuaLanguageServerFactory.getServerInterface`). */
interface WowLuaLanguageServerApi : LanguageServer {
    /**
     * The hover for a class or alias name, as the server shows it for the name in
     * an annotation of [TypeHoverParams.textDocument] (an open document), or null
     * if the name is no type there.
     */
    @JsonRequest("wowlua/typeHover")
    fun typeHover(params: TypeHoverParams): CompletableFuture<Hover?>
}

data class TypeHoverParams(val textDocument: TextDocumentIdentifier, val name: String)
