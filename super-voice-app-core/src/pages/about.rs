// SPDX-License-Identifier: GPL-3.0-only
//! The About page: which Super Voice this is, which daemon it talks to, its
//! license, and where its source is and problems go.
//!
//! The app's own version and commit are the app's to give ([`Build`]), since
//! only its build knows them; the daemon's version is read from the daemon
//! once it answers. Copy details puts all of it on the clipboard, for a
//! problem report.

use crate::daemon::client;

/// The app's build: what it was built from, and where that lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    /// `0.3.0-beta.7`, without a `v`.
    pub version: String,
    /// The commit it was built from, when the build knows.
    pub commit: Option<String>,
    /// That commit's date, `2026-11-25`.
    pub commit_date: Option<String>,
    /// The source repository's address.
    pub repository: String,
}

impl Build {
    /// The commit, shortened to the 7 characters people quote.
    #[must_use]
    pub fn short_commit(&self) -> Option<&str> {
        self.commit
            .as_deref()
            .map(|sha| sha.get(..7).unwrap_or(sha))
    }
}

/// What the page knows of the daemon's version.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DaemonVersion {
    /// Not asked: the daemon hasn't answered since the app started.
    #[default]
    NotRead,
    Reading,
    /// `0.3.0-beta.7`, without a `v`.
    Known(String),
    /// The daemon didn't say: it is too old to, or the read failed.
    Unknown,
}

/// The page's state.
#[derive(Debug, Clone)]
pub struct AboutState {
    pub build: Build,
    pub daemon: DaemonVersion,
}

impl AboutState {
    #[must_use]
    pub fn new(build: Build) -> Self {
        Self {
            build,
            daemon: DaemonVersion::NotRead,
        }
    }
}

/// An address the page links to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// The source repository.
    Source,
    /// Where problems are reported.
    Issues,
    /// The license's text.
    License,
    /// The licenses of the software Super Voice is built with.
    Notices,
    /// The commit this build was made from.
    Commit,
}

/// What happened.
#[derive(Debug, Clone)]
pub enum Input {
    /// The daemon answers, first or again: ask for its version.
    Reached,
    /// The daemon's version, `None` from a daemon too old to say, or why it
    /// couldn't be read.
    DaemonRead(Result<Option<String>, String>),
    /// Copy details was pressed.
    CopyDetails,
    /// A link was pressed.
    Open(Link),
}

/// What the app should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Read the daemon's version ([`run_read_daemon_version`]).
    ReadDaemonVersion,
    /// Put this text on the clipboard.
    Copy(String),
    /// Open this address in the browser.
    OpenUrl(String),
}

/// Something to tell the user. The app says it in its own words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// The details are on the clipboard.
    Copied,
}

/// What an input came to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub effects: Vec<Effect>,
    /// Take the notice down.
    pub clear_notice: bool,
    pub notice: Option<Notice>,
}

/// Take in `input`, and say what to do.
pub fn update(state: &mut AboutState, input: Input) -> Outcome {
    match input {
        Input::Reached => {
            state.daemon = DaemonVersion::Reading;
            Outcome {
                effects: vec![Effect::ReadDaemonVersion],
                ..Outcome::default()
            }
        }
        Input::DaemonRead(Ok(Some(version))) => {
            state.daemon = DaemonVersion::Known(without_v(&version).to_string());
            Outcome::default()
        }
        Input::DaemonRead(Ok(None)) => {
            state.daemon = DaemonVersion::Unknown;
            Outcome::default()
        }
        Input::DaemonRead(Err(e)) => {
            log::debug!("couldn't read the daemon's version: {e}");
            state.daemon = DaemonVersion::Unknown;
            Outcome::default()
        }
        Input::CopyDetails => Outcome {
            effects: vec![Effect::Copy(details(state))],
            clear_notice: true,
            notice: Some(Notice::Copied),
        },
        Input::Open(link) => Outcome {
            effects: url(&state.build, link)
                .map(Effect::OpenUrl)
                .into_iter()
                .collect(),
            ..Outcome::default()
        },
    }
}

/// `link`'s address, or `None` for the commit of a build that doesn't know
/// its own.
#[must_use]
pub fn url(build: &Build, link: Link) -> Option<String> {
    let repository = build.repository.trim_end_matches('/');
    Some(match link {
        Link::Source => repository.to_string(),
        Link::Issues => format!("{repository}/issues"),
        Link::License => format!("{repository}/blob/main/LICENSE"),
        Link::Notices => format!("{repository}/blob/main/NOTICE"),
        Link::Commit => format!("{repository}/commit/{}", build.commit.as_deref()?),
    })
}

/// What Copy details puts on the clipboard: one fact a line, in English,
/// since it goes into a problem report.
#[must_use]
pub fn details(state: &AboutState) -> String {
    let build = &state.build;
    let mut lines = vec![format!("Super Voice {}", build.version)];
    if let Some(commit) = build.short_commit() {
        match &build.commit_date {
            Some(date) => lines.push(format!("Commit {commit} ({date})")),
            None => lines.push(format!("Commit {commit}")),
        }
    }
    lines.push(match &state.daemon {
        DaemonVersion::Known(version) => format!("Daemon {version}"),
        _ => "Daemon unknown".to_string(),
    });
    lines.push(format!(
        "{} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    lines.join("\n")
}

/// `version` without a leading `v`, as the daemon may give it.
fn without_v(version: &str) -> &str {
    version.strip_prefix('v').unwrap_or(version)
}

/// Read the daemon's version: the one it reports with its update status.
pub async fn run_read_daemon_version() -> Input {
    Input::DaemonRead(
        client::get_update_status()
            .await
            .map(|status| status.map(|s| s.current_version))
            .map_err(|e| e.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build() -> Build {
        Build {
            version: "0.3.0-beta.7".into(),
            commit: Some("5d2ea8592abcdef".into()),
            commit_date: Some("2026-11-25".into()),
            repository: "https://github.com/super-libre/super-voice".into(),
        }
    }

    #[test]
    fn the_daemons_version_is_read_once_it_answers() {
        let mut state = AboutState::new(build());
        assert_eq!(state.daemon, DaemonVersion::NotRead);
        let outcome = update(&mut state, Input::Reached);
        assert_eq!(outcome.effects, [Effect::ReadDaemonVersion]);
        assert_eq!(state.daemon, DaemonVersion::Reading);
        update(
            &mut state,
            Input::DaemonRead(Ok(Some("v0.3.0-beta.6".into()))),
        );
        assert_eq!(state.daemon, DaemonVersion::Known("0.3.0-beta.6".into()));
        update(&mut state, Input::DaemonRead(Ok(None)));
        assert_eq!(state.daemon, DaemonVersion::Unknown);
        update(&mut state, Input::DaemonRead(Err("gone".into())));
        assert_eq!(state.daemon, DaemonVersion::Unknown);
    }

    #[test]
    fn each_link_goes_to_its_page_in_the_repository() {
        let build = build();
        let base = "https://github.com/super-libre/super-voice";
        assert_eq!(url(&build, Link::Source).as_deref(), Some(base));
        assert_eq!(url(&build, Link::Issues), Some(format!("{base}/issues")));
        assert_eq!(
            url(&build, Link::License),
            Some(format!("{base}/blob/main/LICENSE"))
        );
        assert_eq!(
            url(&build, Link::Notices),
            Some(format!("{base}/blob/main/NOTICE"))
        );
        assert_eq!(
            url(&build, Link::Commit),
            Some(format!("{base}/commit/5d2ea8592abcdef"))
        );
        let no_commit = Build {
            commit: None,
            ..build
        };
        assert_eq!(url(&no_commit, Link::Commit), None);
    }

    #[test]
    fn a_link_opens_its_address() {
        let mut state = AboutState::new(build());
        assert_eq!(
            update(&mut state, Input::Open(Link::Issues)).effects,
            [Effect::OpenUrl(
                "https://github.com/super-libre/super-voice/issues".into()
            )]
        );
        state.build.commit = None;
        assert!(
            update(&mut state, Input::Open(Link::Commit))
                .effects
                .is_empty()
        );
    }

    #[test]
    fn copy_details_copies_every_version_and_says_so() {
        let mut state = AboutState::new(build());
        state.daemon = DaemonVersion::Known("0.3.0-beta.6".into());
        let outcome = update(&mut state, Input::CopyDetails);
        let [Effect::Copy(text)] = &outcome.effects[..] else {
            panic!("one copy: {outcome:?}");
        };
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "Super Voice 0.3.0-beta.7");
        assert_eq!(lines[1], "Commit 5d2ea85 (2026-11-25)");
        assert_eq!(lines[2], "Daemon 0.3.0-beta.6");
        assert_eq!(lines.len(), 4);
        assert_eq!(outcome.notice, Some(Notice::Copied));
        assert!(outcome.clear_notice);
    }
}
