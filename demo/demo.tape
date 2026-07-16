# Renders docs/demo.gif — run from the repo root:
#   cargo build --release
#   rm -f ~/.config/databricks-tui/config.json
#   PATH="$PWD/demo/shim:$PATH" vhs demo/demo.tape
# (demo/shim/databricks is a symlink to fake-databricks so the app
# talks to canned data instead of a real workspace.)

Output docs/demo.gif
Output docs/demo.mp4

Set FontSize 14
Set Width 1400
Set Height 800
Set Padding 12
Set Theme "Catppuccin Mocha"
Set TypingSpeed 45ms

Hide
Type "./target/release/databricks-tui --theme catppuccin-mocha"
Enter
Sleep 2.5s
Show

# Splash settles into the dashboard — take it in.
Sleep 4s

# --- Debug a failed job run, in the terminal ---
Type "l"
Sleep 1s
Type "j"
Sleep 600ms
Enter
Sleep 2s
Enter
Sleep 2.5s
Type "h"
Sleep 4s
Escape
Sleep 700ms
Escape
Sleep 900ms

# --- Browse Unity Catalog and preview a table with SQL ---
Type "l"
Sleep 300ms
Type "l"
Sleep 300ms
Type "l"
Sleep 300ms
Type "l"
Sleep 800ms
Enter
Sleep 1.5s
Enter
Sleep 1.5s
Type "j"
Sleep 600ms
Type ":"
Sleep 2s
Enter
Sleep 1.2s
Enter
Sleep 3.5s
Escape
Sleep 900ms

# --- Table lineage as a tree ---
Type "L"
Sleep 4s
Escape
Sleep 900ms

# --- 14 days of spend with dollar estimates and top spenders ---
Type "$"
Sleep 4.5s
Escape
Sleep 900ms

# --- Eight built-in themes ---
Type "t"
Sleep 1.3s
Type "t"
Sleep 1.3s
Type "t"
Sleep 1.8s

Type "q"
Sleep 500ms
