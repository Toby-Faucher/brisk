# Launch options spike: what to check on the real games VM

The code is done and tested with fake spawns. Nothing has run a real wrapper yet. Run this list once on the
games VM, with Steam and Dolphin installed, then fix the notes below.

Setup: install `mangohud`, `gamemode` and `gamescope` (system packages). Set one Steam game's launch option to
the line from its brisk panel. Start with only MangoHud on.

1. `brisk-hostd wrap steam:<appid> -- /bin/echo a "b c"` prints `a b c` as two arguments and logs one line.
2. Start the game from Steam. Does the wrapper line appear in `ps -ef` above the game? Does `SteamAppId` show
   in the game's environment?
3. Does Steam pass `%command%` intact? For a Proton game it is
   `.../proton waitforexitandrun /path/game.exe`. For a native game it is the game binary. Check argument
   boundaries with a path that has a space in it.
4. MangoHud overlay: is it visible in the browser stream? Does it also show on the host screen? Does the
   Vulkan or the OpenGL path matter (try one of each)?
5. GameMode: `gamemoded -s` reports the game registered while it runs.
6. gamescope under the streaming session: does it start at all under the Sunshine session (nested compositor
   inside the sway session)? Does the stream capture its window? Does `-f` behave? Does Quit still find and end
   the game with gamescope above it?
7. Env vars: set `DXVK_HUD=fps` and check it reaches a Proton game.
8. Quit: with wrappers on, does the Quit button still end the game and does the running badge clear?
9. Dolphin: Play with MangoHud and with gamescope on. Does the window class still match for focus and
   fullscreen? Does the process match (running badge, playtime) still see Dolphin?
10. Failure paths: remove a wrapper (rename its binary) and start a game: it must start plain. Switch off Allow
    custom commands with a custom command saved: the game must start plain and the log must say why.
11. Does Steam's "Launch Options" field accept the long line, and does Steam keep it after a restart?

Record what differs. If a wrapper cannot run under the stream (likely gamescope), hide it in Settings rather
than leaving a switch that does nothing.
