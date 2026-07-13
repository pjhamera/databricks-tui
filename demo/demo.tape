# Renders docs/demo.gif — run from the repo root:
#   cargo build --release
#   PATH="$PWD/demo/shim:$PATH" vhs demo/demo.tape
# (demo/shim/databricks is a symlink to fake-databricks so the app
# talks to canned data instead of a real workspace.)

Output docs/demo.gif

Set FontSize 13
Set Width 1400
Set Height 800
Set Padding 12
Set Theme "Catppuccin Mocha"
Set TypingSpeed 40ms

Hide
Type "./target/release/databricks-tui --theme catppuccin-mocha"
Enter
Sleep 2.5s
Show

# Splash settles into the six-pane dashboard.
Sleep 3.5s

# Drill into a job, then into its failed run to see the real error.
Type "l"
Sleep 1s
Enter
Sleep 2s
Enter
Sleep 2.5s
Type "h"
Sleep 3.5s
Escape
Sleep 800ms
Escape
Sleep 800ms

# SQL console prefilled from the selected catalog table
# (Jobs → h → Compute → h → Unity Catalog).
Type "h"
Sleep 500ms
Type "h"
Sleep 800ms
Enter
Sleep 1.5s
Enter
Sleep 1.5s
Type "j"
Sleep 500ms
Type ":"
Sleep 1.5s
Enter
Sleep 1s
Enter
Sleep 3s
Escape
Sleep 800ms

# Workspace costs with dollar estimates and top spenders.
Type "$"
Sleep 4s
Escape
Sleep 800ms

# Theme cycle finale.
Type "t"
Sleep 1.2s
Type "t"
Sleep 1.2s
Type "t"
Sleep 1.5s

Type "q"
Sleep 500ms
