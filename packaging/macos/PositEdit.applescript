-- Posit Edit macOS wrapper (neovim-style).
-- Launches the posit-edit CLI in a terminal; does not embed the TUI binary.
-- Prefer Ghostty / Kitty / iTerm2 / WezTerm / Alacritty when installed, else Terminal.app.
-- Uses shell `open` so the applet compiles without third-party scripting dictionaries.

on resolveCli()
	set candidates to {"posit-edit", "pst"}
	set homePath to POSIX path of (path to home folder)
	set pathDirs to {"/opt/homebrew/bin", "/usr/local/bin", homePath & ".local/bin", "/run/current-system/sw/bin"}
	repeat with exeName in candidates
		try
			set found to do shell script "/usr/bin/env zsh -lc " & quoted form of ("command -v " & (exeName as text))
			if found is not "" then return found
		end try
		repeat with dir in pathDirs
			set candidate to (dir as text) & "/" & (exeName as text)
			try
				do shell script "/bin/test -x " & quoted form of candidate
				return candidate
			end try
		end repeat
	end repeat
	error "posit-edit CLI not found on PATH. Install the CLI (cargo/nix) first."
end resolveCli

on shellQuote(p)
	return quoted form of (p as text)
end shellQuote

on buildCommand(theFiles)
	set cli to resolveCli()
	set cmd to shellQuote(cli)
	repeat with f in theFiles
		set cmd to cmd & " " & shellQuote(POSIX path of f)
	end repeat
	return cmd
end buildCommand

on appExists(appName)
	set homePath to POSIX path of (path to home folder)
	try
		do shell script "/bin/test -d /Applications/" & quoted form of ((appName as text) & ".app") & " -o -d " & quoted form of (homePath & "Applications/" & (appName as text) & ".app")
		return true
	end try
	return false
end appExists

on preferredTerminal()
	try
		set pref to do shell script "defaults read io.github.aspauldingcode.PositEdit terminal 2>/dev/null || true"
		if pref is "ghostty" and appExists("Ghostty") then return "Ghostty"
		if pref is "kitty" and appExists("kitty") then return "kitty"
		if pref is "iterm2" and appExists("iTerm") then return "iTerm"
		if pref is "wezterm" and appExists("WezTerm") then return "WezTerm"
		if pref is "alacritty" and appExists("Alacritty") then return "Alacritty"
		if pref is "terminal" then return "Terminal"
	end try
	if appExists("Ghostty") then return "Ghostty"
	if appExists("kitty") then return "kitty"
	if appExists("iTerm") then return "iTerm"
	if appExists("WezTerm") then return "WezTerm"
	if appExists("Alacritty") then return "Alacritty"
	return "Terminal"
end preferredTerminal

on launchInTerminal(cmd)
	set termName to preferredTerminal()
	set zshCmd to "/bin/zsh -lc " & quoted form of cmd
	if termName is "Ghostty" then
		do shell script "open -na Ghostty --args -e /bin/zsh -lc " & quoted form of cmd
	else if termName is "kitty" then
		do shell script "open -na kitty --args /bin/zsh -lc " & quoted form of cmd
	else if termName is "iTerm" then
		-- Avoid iTerm's scripting dictionary (may be absent at compile time).
		do shell script "open -na iTerm --args " & quoted form of zshCmd
	else if termName is "WezTerm" then
		do shell script "open -na WezTerm --args start -- /bin/zsh -lc " & quoted form of cmd
	else if termName is "Alacritty" then
		do shell script "open -na Alacritty --args -e /bin/zsh -lc " & quoted form of cmd
	else
		-- Terminal.app: `open` cannot pass a command reliably; use a tiny osascript.
		do shell script "osascript -e " & quoted form of ("tell application \"Terminal\" to activate") & " -e " & quoted form of ("tell application \"Terminal\" to do script " & quoted form of cmd)
	end if
end launchInTerminal

on open theFiles
	set cmd to buildCommand(theFiles)
	launchInTerminal(cmd)
end open

on run
	set cmd to buildCommand({})
	launchInTerminal(cmd)
end run
