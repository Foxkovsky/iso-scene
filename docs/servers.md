# Language server and MCP server

Both run from the `iso-scene` binary over stdin/stdout and need nothing
else installed.

## Language server: `iso-scene lsp`

For `.scene`, `.model` and `.walls` files:

- **diagnostics** on open, on every change and on save, from the same parser
  and evaluator as the CLI (the first error, with its code and range; an
  error inside an included model sits on the `include` line and names the
  file and position);
- **hover**: the header of an object, material or animation (also from
  included models), or the spec's words on a keyword or shape;
- **go to definition** of objects, materials and animations, also into
  included `.model` files;
- **document symbols**: objects, materials, animations;
- **completion**: keywords and shapes (with snippets) from the spec, object
  names after `place`/`use`, materials after `material`, anchors after
  `anchor`, views after `render`.

While a line is being typed, declarations above it keep working: the server
falls back to the text up to the last complete `;` or `}`.

The project root (the directory includes must stay inside) is the client's
`rootUri`, or the server's working directory.

### Neovim

```lua
vim.filetype.add({ extension = { scene = "isoscene", model = "isoscene", walls = "isoscene" } })
vim.api.nvim_create_autocmd("FileType", {
  pattern = "isoscene",
  callback = function()
    vim.lsp.start({ name = "iso-scene", cmd = { "iso-scene", "lsp" }, root_dir = vim.fn.getcwd() })
  end,
})
```

### Helix (`languages.toml`)

```toml
[language-server.iso-scene]
command = "iso-scene"
args = ["lsp"]

[[language]]
name = "iso-scene"
scope = "source.iso-scene"
file-types = ["scene", "model", "walls"]
language-servers = ["iso-scene"]
```

### VS Code

The extension in `editors/vscode` still has its own hover and completion;
switching it to `iso-scene lsp` needs the `vscode-languageclient` package and
is not done yet.
