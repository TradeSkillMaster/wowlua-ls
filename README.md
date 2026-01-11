# Motivation
I am frustrated by the existing tooling

I want a language server that is fully type-free (with optional types in comments), and still gives out decent results.

Plus it should understand the connection between the WoW toc, XML and lua and be able to mix stuff into frames properly

# Action Plan
My latest problem is how to efficiently track types across assignments and operators applying 😄.

For now I'm focussing on basic types (number, function, string). And then table (and tracking what fields are assigned to it) before trying to expand it to multi-file
