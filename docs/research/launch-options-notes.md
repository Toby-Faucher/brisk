# Launch options: the Steam `brisk-wrap` shim

The shim is not built yet. `crates/brisk-hostd/src/launch.rs` is the logic it would reuse.

1. Each Steam game gets one Launch Option, set once: `brisk-wrap %command%`.
2. Steam runs `brisk-wrap <the real command...>` with `SteamAppId` in its environment.
3. The shim reads `launch.json` from the brisk data dir (the same file hostd writes), never from arguments.
4. It looks up the game by `SteamAppId`, then calls `effective(defaults, per_game_override)`.
5. It reads `allow_custom_commands` from Settings and calls `validate(&opts, allow_custom)`.
6. On any problem it logs the reasons to stderr and runs the game with no options. A bad setting never blocks a launch.
7. On success it calls `build(&validated, &args_after_brisk_wrap)` and gets `{program, args, env}`.
8. It sets the env pairs on its own process, then `exec`s the program with the args. No shell, no `sh -c`.
9. The wrapper order is fixed: gamescope, then gamemoderun, then mangohud, then the custom command, then the game.
10. Spike (half a day, on a machine with Steam): set the option on one game by hand, and print the shim's argv instead of exec'ing.
11. Check that `%command%` arrives as separate argv entries, including the Proton `waitforexitandrun` form.
12. Check that `SteamAppId` is set for native and Proton titles, and that gamescope inside Steam's runtime container starts at all.
13. Check that Quit still finds the game: the process tree has wrappers above it now.
14. Then decide between writing the one-time option into `localconfig.vdf` (Steam closed) and showing the line to paste.
15. Sharing the code: the shim and hostd need the same module, so it moves to a small shared crate, or the shim is a hostd subcommand.
