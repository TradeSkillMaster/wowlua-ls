package com.tradeskillmaster.wowluals.lsp4ij

import java.net.URLDecoder
import java.net.URLEncoder
import java.nio.charset.StandardCharsets

/**
 * Clickable types in hover popups.
 *
 * The server ends a hover's markdown with a `Go to:` line linking each class and
 * alias the hover's signature names to the line that declares it
 * (`file://…#L<line>`; `lua_hover` in the server's `handlers.rs`). Markdown can't
 * put a link inside the fenced signature, so other editors show that line as is.
 * Here [WowLuaHoverFeature] drops it and links the names inside the highlighted
 * signature instead, to a [WowLuaTypeDocumentationTarget] that shows the type's
 * own hover in the same popup.
 */
internal object HoverTypeLinks {
    /** A type a hover links: its name and the `file://…#L<line>` URL declaring it. */
    data class TypeLink(val name: String, val declarationUrl: String)

    /**
     * What a linked type in the popup resolves to: the type, read in [contextUri]'s
     * file, of the project whose `locationHash` is [projectHash].
     */
    data class LinkTarget(val name: String, val projectHash: String, val contextUri: String, val declarationUrl: String)

    /** The scheme of the links [linkify] adds, which [WowLuaTypeLinkHandler] resolves. */
    const val SCHEME = "wowlua-type:"

    // The server's separator and label (`HOVER_TYPE_LINKS_LABEL`), and its
    // "[`Name`](url)" links joined by " · ". Its URLs have no `)` or whitespace.
    private const val FOOTER_START = "\n\n---\n\nGo to: "
    private const val LINK_SEPARATOR = " · "
    private val FOOTER_LINK = Regex("""\[`([^`]+)`]\(([^)\s]+)\)""")

    /**
     * Splits the server's `Go to:` line off a hover's [markdown]: the markdown
     * without it, and the links it lists. A hover without one comes back whole.
     */
    fun splitFooter(markdown: String): Pair<String, List<TypeLink>> {
        val start = markdown.lastIndexOf(FOOTER_START)
        if (start < 0) return markdown to emptyList()
        val links = markdown.substring(start + FOOTER_START.length).trimEnd()
            .split(LINK_SEPARATOR)
            .map { part ->
                val match = FOOTER_LINK.matchEntire(part) ?: return markdown to emptyList()
                TypeLink(match.groupValues[1], match.groupValues[2])
            }
        return markdown.substring(0, start) to links
    }

    /**
     * The `href` of a link to [link]'s type, read in the file at [contextUri] of the
     * project whose `locationHash` is [projectHash]. The project rides along because
     * the file can be one no project contains (a stub file opened from
     * go-to-definition).
     */
    fun href(link: TypeLink, projectHash: String, contextUri: String): String =
        SCHEME + encode(link.name) + "?project=" + encode(projectHash) + "&in=" + encode(contextUri) +
            "&at=" + encode(link.declarationUrl)

    /** The target of an [href], or null for any other URL. */
    fun parseHref(href: String): LinkTarget? {
        if (!href.startsWith(SCHEME)) return null
        val query = href.substring(SCHEME.length).split('?', limit = 2)
        if (query.size != 2) return null
        val params = query[1].split('&').associate {
            val (key, value) = it.split('=', limit = 2).let { kv -> kv[0] to kv.getOrElse(1) { "" } }
            key to decode(value)
        }
        val projectHash = params["project"] ?: return null
        val contextUri = params["in"] ?: return null
        val declarationUrl = params["at"] ?: return null
        return LinkTarget(decode(query[0]), projectHash, contextUri, declarationUrl)
    }

    /**
     * The file URI and 0-based line a `file://…#L<line>` declaration URL names
     * (the line VS Code and LSP4IJ open it at), or null if it names none.
     */
    fun declarationLine(declarationUrl: String): Pair<String, Int>? {
        val at = declarationUrl.lastIndexOf("#L")
        if (at < 0) return null
        val line = declarationUrl.substring(at + 2).toIntOrNull() ?: return null
        return declarationUrl.substring(0, at) to (line - 1).coerceAtLeast(0)
    }

    /**
     * Links each word of the signature — [html]'s first `<pre>` block, ahead of
     * the doc, whose own code examples are left alone — that [typeWordRanges]
     * finds and [hrefs] has a link for. A word the highlighting split across tags
     * (`Lib-2.0`) is linked piece by piece, so the links nest inside the colored
     * spans rather than across them.
     */
    fun linkify(html: String, hrefs: Map<String, String>): String {
        if (hrefs.isEmpty()) return html
        val open = html.indexOf("<pre")
        if (open < 0) return html
        val contentStart = html.indexOf('>', open) + 1
        val close = html.indexOf("</pre>", contentStart)
        if (contentStart <= 0 || close < 0) return html
        val inserts = sortedMapOf<Int, StringBuilder>()
        fun insert(at: Int, text: String) {
            inserts.getOrPut(at) { StringBuilder() }.append(text)
        }
        val text = VisibleText(html, contentStart, close)
        for (range in typeWordRanges(text.chars)) {
            val href = hrefs[text.chars.substring(range)] ?: continue
            val anchor = "<a href=\"" + escapeAttribute(href) + "\">"
            // One link per run of characters the HTML has side by side.
            var runStart = range.first
            for (i in range) {
                if (i == range.last || text.sources[i + 1] != text.sources[i] + 1) {
                    insert(text.sources[runStart], anchor)
                    insert(text.sources[i] + 1, "</a>")
                    runStart = i + 1
                }
            }
        }
        if (inserts.isEmpty()) return html
        val out = StringBuilder(html.length + inserts.values.sumOf { it.length })
        var copied = 0
        for ((at, text) in inserts) {
            out.append(html, copied, at).append(text)
            copied = at
        }
        return out.append(html, copied, html.length).toString()
    }

    /**
     * The ranges of [text] where a type can stand: the server's
     * `signature_type_words` rules (`queries/hover.rs`), which pick the names its
     * `Go to:` line lists, so keep the two in sync. A word is an identifier, with
     * `.` and `-` joining word characters (`Lib-2.0.Column`); words in `--`
     * comments and string literals don't count, nor do a parameter list's names
     * (`fun(self, frame)`, typed or not), field labels (`name:` / `name?:` before a
     * space or line end), a table's keys (`Line = 3` inside `{}`), method names
     * (right after `:`) or function names (right before `(`).
     */
    fun typeWordRanges(text: String): List<IntRange> {
        fun isWord(c: Char) = c.isAsciiLetterOrDigit() || c == '_'
        // A parameter list's `(` on the stack of open brackets: one right after a
        // name (`fun(`, `Frame:SetPoint(`), unlike a grouping `(` or `(local)`.
        val params = '\u0000'
        val open = ArrayDeque<Char>()
        // At the start of a parameter: after a parameter list's `(` or its `,`.
        var parameterStart = false
        val words = mutableListOf<IntRange>()
        var i = 0
        while (i < text.length) {
            val c = text[i]
            when {
                c == '-' && text.getOrNull(i + 1) == '-' -> {
                    while (i < text.length && text[i] != '\n') i++
                }
                // A return arrow, whose `>` closes nothing.
                c == '-' && text.getOrNull(i + 1) == '>' -> i += 2
                c == '"' || c == '\'' || c == '`' -> {
                    i++
                    while (i < text.length && text[i] != c && text[i] != '\n') {
                        i += if (text[i] == '\\') 2 else 1
                    }
                    i++
                }
                c in '0'..'9' -> {
                    while (i < text.length && (isWord(text[i]) || text[i] == '.')) i++
                }
                c.isAsciiLetter() || c == '_' -> {
                    val start = i
                    while (true) {
                        while (i < text.length && isWord(text[i])) i++
                        if (i + 1 < text.length && (text[i] == '.' || text[i] == '-') && isWord(text[i + 1])) {
                            i++
                        } else {
                            break
                        }
                    }
                    val parameter = parameterStart
                    parameterStart = false
                    val methodName = start > 0 && text[start - 1] == ':'
                    val colon = if (text.getOrNull(i) == '?') i + 1 else i
                    val label = text.getOrNull(colon) == ':' &&
                        text.getOrNull(colon + 1).let { it == null || it.isWhitespace() }
                    val functionName = text.getOrNull(i) == '('
                    val after = text.substring(i).trimStart(' ', '\t')
                    val key = open.lastOrNull() == '{' && after.startsWith('=') && !after.startsWith("==")
                    if (!parameter && !methodName && !label && !functionName && !key) words += start until i
                }
                else -> {
                    when (c) {
                        '(' -> {
                            val isParams = i > 0 && isWord(text[i - 1])
                            open.addLast(if (isParams) params else c)
                            parameterStart = isParams
                        }
                        '<', '{', '[' -> {
                            open.addLast(c)
                            parameterStart = false
                        }
                        ')', '>', '}', ']' -> {
                            open.removeLastOrNull()
                            parameterStart = false
                        }
                        ',' -> parameterStart = open.lastOrNull() == params
                        else -> if (!c.isWhitespace()) parameterStart = false
                    }
                    i++
                }
            }
        }
        return words
    }

    /**
     * The text a stretch of HTML shows, with where each character came from: tags
     * dropped (a `<br>` reads as a line break), entities decoded.
     */
    private class VisibleText(html: String, start: Int, end: Int) {
        val chars: String
        /** The HTML index of each character of [chars]. */
        val sources: IntArray

        init {
            val text = StringBuilder()
            val from = ArrayList<Int>()
            var i = start
            while (i < end) {
                val c = html[i]
                when {
                    c == '<' -> {
                        val close = html.indexOf('>', i).takeIf { it in i until end } ?: end
                        val tag = html.substring(i + 1, close).trimStart('/').lowercase()
                        if (tag == "br" || tag.startsWith("br ") || tag.startsWith("br/")) {
                            text.append('\n')
                            from += close
                        }
                        i = close + 1
                    }
                    c == '&' -> {
                        val semi = html.indexOf(';', i).takeIf { it in i + 1..minOf(i + 10, end - 1) }
                        if (semi == null) {
                            text.append(c)
                            from += i
                            i++
                        } else {
                            text.append(decodeEntity(html.substring(i + 1, semi)))
                            from += i
                            i = semi + 1
                        }
                    }
                    else -> {
                        text.append(c)
                        from += i
                        i++
                    }
                }
            }
            chars = text.toString()
            sources = from.toIntArray()
        }
    }

    /** An entity's character; one the signature rules don't care about reads as a space. */
    private fun decodeEntity(name: String): Char = when {
        name == "lt" -> '<'
        name == "gt" -> '>'
        name == "amp" -> '&'
        name == "quot" -> '"'
        name == "apos" -> '\''
        name.startsWith("#x") || name.startsWith("#X") -> name.substring(2).toIntOrNull(16)?.toChar() ?: ' '
        name.startsWith("#") -> name.substring(1).toIntOrNull()?.toChar() ?: ' '
        else -> ' '
    }

    private fun Char.isAsciiLetter() = this in 'a'..'z' || this in 'A'..'Z'
    private fun Char.isAsciiLetterOrDigit() = isAsciiLetter() || this in '0'..'9'

    private fun escapeAttribute(value: String) = value.replace("&", "&amp;").replace("\"", "&quot;")
    private fun encode(value: String) = URLEncoder.encode(value, StandardCharsets.UTF_8)
    private fun decode(value: String) = URLDecoder.decode(value, StandardCharsets.UTF_8)
}
