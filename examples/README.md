# Example MCP client configs

Replace `YOUR_USERNAME` with your account name. Install the binary first:
`cargo install --git https://github.com/shubhxho/jlcpcb-mcp` (it lands in `~/.cargo/bin`).
If the target file already exists, merge the `"jlcpcb"` entry into its existing `mcpServers` object
instead of overwriting the file. Restart the client afterwards.

| Example file | Copy to |
|---|---|
| `claude-desktop/macos.claude_desktop_config.json` | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| `claude-desktop/windows.claude_desktop_config.json` | `%APPDATA%\Claude\claude_desktop_config.json` (usually `C:\Users\YOUR_USERNAME\AppData\Roaming\Claude\claude_desktop_config.json`) |
| `claude-desktop/linux.claude_desktop_config.json` | `~/.config/Claude/claude_desktop_config.json` (Anthropic ships no official Linux build; this is the path used by community Linux builds) |
| `claude-desktop/with-official-api.claude_desktop_config.json` | same path as your OS above; adds the optional `JLCPCB_*` credentials |
| `cursor/project/.cursor/mcp.json` | `<your-project>/.cursor/mcp.json` (project scope, safe to commit) |
| `cursor/global/mcp.json` | `~/.cursor/mcp.json` (all projects; Windows: `%USERPROFILE%\.cursor\mcp.json`) |

Claude Desktop is a GUI app and does not read your shell `PATH`, so its examples use the absolute
path to the binary. Cursor normally finds `jlcpcb-mcp` on `PATH`; if it doesn't, use the absolute path
there too.
