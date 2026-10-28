//! Where a game comes from. Handlers and the Quitter ask a [`GameSource`] instead of looking at
//! an id range: Steam runs a game through its client and finds it by its launcher reaper, an
//! emulator is its own process with a window class. Listing stays with each source's own loader
//! (`steam::load_games_with`, `emu::games`) because Steam's needs the network.

use crate::emu;
use crate::game::{GameId, Source};
use crate::host::Runner;
use crate::running::{self, Proc};
use std::collections::HashSet;
use std::path::Path;

/// What a quit acts on: the top processes, their descendants, and the window class when the
/// windows are found by class instead of by pid.
#[derive(Debug, PartialEq)]
pub(crate) struct Scope {
    pub reapers: HashSet<u32>,
    pub kids: HashSet<u32>,
    pub class: Option<&'static str>,
}

pub(crate) trait GameSource: Sync {
    /// Starts the game, detached; an emulator already running it is left alone and counts as
    /// started. Blocking.
    fn launch(
        &self,
        runner: &dyn Runner,
        proc: &Path,
        data_dir: &Path,
        id: &GameId,
        env: &[(String, String)],
    ) -> bool;
    /// `None`: this source has nothing to install.
    fn install(&self, runner: &dyn Runner, id: &GameId) -> Option<bool>;
    /// Runs after a successful launch: the last-played stamp the source keeps itself.
    fn after_launch(&self, data_dir: &Path, id: &GameId);
    /// The running game's processes; `None` when it is not running.
    fn scope(&self, ps: &[Proc], data_dir: &Path, id: &GameId) -> Option<Scope>;
    /// The window class to focus after a launch; `None` = find the window through `scope`.
    fn focus_class(&self, data_dir: &Path, id: &GameId) -> Option<&'static str>;
}

pub(crate) struct SteamSource;
pub(crate) struct EmuSource;

pub(crate) fn of(id: &GameId) -> &'static dyn GameSource {
    match id.source() {
        Source::Steam => &SteamSource,
        Source::Emu => &EmuSource,
    }
}

impl GameSource for SteamSource {
    fn launch(
        &self,
        runner: &dyn Runner,
        _: &Path,
        _: &Path,
        id: &GameId,
        _: &[(String, String)],
    ) -> bool {
        id.steam_appid()
            .is_some_and(|n| crate::stream::steam(runner, "rungameid", n))
    }
    fn install(&self, runner: &dyn Runner, id: &GameId) -> Option<bool> {
        Some(
            id.steam_appid()
                .is_some_and(|n| crate::stream::steam(runner, "install", n)),
        )
    }
    fn after_launch(&self, _: &Path, _: &GameId) {}
    fn scope(&self, ps: &[Proc], _: &Path, id: &GameId) -> Option<Scope> {
        running::tree(ps, id.steam_appid()?).map(|(reapers, kids)| Scope {
            reapers,
            kids,
            class: None,
        })
    }
    fn focus_class(&self, _: &Path, _: &GameId) -> Option<&'static str> {
        None
    }
}

impl GameSource for EmuSource {
    fn launch(
        &self,
        runner: &dyn Runner,
        proc: &Path,
        data_dir: &Path,
        id: &GameId,
        env: &[(String, String)],
    ) -> bool {
        emu::launch(runner, proc, &emu::load(data_dir), id, env)
    }
    fn install(&self, _: &dyn Runner, _: &GameId) -> Option<bool> {
        None
    }
    fn after_launch(&self, data_dir: &Path, id: &GameId) {
        emu::mark_played(data_dir, id);
    }
    /// The emulator's own processes, no descendants, and its window class.
    fn scope(&self, ps: &[Proc], data_dir: &Path, id: &GameId) -> Option<Scope> {
        let recs = emu::load(data_dir);
        let r = emu::find(&recs, id)?;
        let pids = emu::pids(ps, r);
        let class = crate::registry::get(&r.emulator)?.window_class;
        (!pids.is_empty()).then_some(Scope {
            reapers: pids,
            kids: HashSet::new(),
            class: Some(class),
        })
    }
    fn focus_class(&self, data_dir: &Path, id: &GameId) -> Option<&'static str> {
        let recs = emu::load(data_dir);
        crate::registry::get(&emu::find(&recs, id)?.emulator).map(|e| e.window_class)
    }
}
