// SPDX-License-Identifier: GPL-3.0-only
//! What the app's About window says: its version, its repository, and the
//! commit it was built from, as the settings app's About page.

/// The repository, which is the settings app's too.
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// What the About window shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct About {
    /// "v0.9.0".
    pub version: String,
    /// The repository's web address.
    pub repository: String,
    /// The commit's hash, shortened to seven characters.
    pub short_commit: String,
    /// The commit's date, as git gives it.
    pub commit_date: String,
    /// The commit's page in the repository.
    pub commit_url: String,
}

/// What the About window shows.
#[must_use]
#[uniffi::export]
pub fn about() -> About {
    let hash = env!("VERGEN_GIT_SHA");
    About {
        version: crate::updates::app_version(),
        repository: REPOSITORY.to_string(),
        short_commit: hash.chars().take(7).collect(),
        commit_date: env!("VERGEN_GIT_COMMIT_DATE").to_string(),
        commit_url: format!("{REPOSITORY}/commits/{hash}"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_commit_links_into_the_repository() {
        let about = super::about();
        assert!(about.commit_url.starts_with(&about.repository));
        assert!(about.commit_url.contains(&about.short_commit));
        assert!(about.short_commit.chars().count() <= 7);
    }
}
