Output docs/demo.gif
Set FontSize 14
Set Width 1200
Set Height 700
Set Padding 20
Set Theme Dracula

Type "# 🧠 AIKD Demo"
Enter
Sleep 1s

Type "# Install AIKD"
Enter
Type "curl -sSf https://raw.githubusercontent.com/gelutjari/aikd/main/install.sh | bash"
Enter
Sleep 3s

Type "# Initialize project"
Enter
Type "aikd init"
Enter
Sleep 2s

Type "# Scan codebase"
Enter
Type "aikd scan"
Enter
Sleep 3s

Type "# Search for authentication"
Enter
Type "aikd query \"authentication\" --limit 3"
Enter
Sleep 3s

Type "# Hybrid search (semantic + keyword)"
Enter
Type "aikd query \"how does login work\" --hybrid --limit 3"
Enter
Sleep 3s

Type "# Get system status"
Enter
Type "aikd status"
Enter
Sleep 2s

Type "# View statistics"
Enter
Type "aikd stats"
Enter
Sleep 2s
