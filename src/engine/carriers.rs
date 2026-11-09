//! Which row carries which GitHub repo: remote identity by owner and name,
//! and the local clones that carry a repo no row claimed.

use super::status::tilde_display;
use crate::model::{self, RepoLocation, RepoStatus};
pub(crate) use crate::model::{RemoteKey, carrier_key};
use crate::{discovery, github};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Whether `entry` is the GitHub repo a row or clone known as `owner`/`name`
/// stands for. Names compare case-insensitively, as GitHub does. With an
/// owner, only an entry of that owner (case-insensitive) matches; an entry
/// without one does not. With no owner, the name alone decides, whatever the
/// entry's owner: that is the legacy path of a payload without `login` (m2
/// and D1 in `docs/repository-identity-design.md`). Decided through
/// `model::identity_key`.
pub(crate) fn names_remote(owner: Option<&str>, name: &str, entry: &github::GithubRepo) -> bool {
    let ours = model::identity_key(owner, name);
    let theirs = model::identity_key(entry.owner.as_deref(), &entry.name);
    match owner {
        Some(_) => ours == theirs,
        None => ours.1 == theirs.1,
    }
}

/// Whether `repo` is the row that carries `entry`'s GitHub fields, the mark
/// the load left (D6, section 3.5 of `docs/repository-identity-design.md`).
/// The poll and `remotes_that_moved` follow it; nothing is re-chosen here.
///
/// A Primary, Elsewhere or NotCloned row with an owner matches only an entry
/// of that owner and name, so an entry without an owner never writes it. One
/// with no owner matches any entry of its name (legacy, `names_remote`). A
/// LocalOnly row matches only when the load made it a carrier by giving it
/// an owner, and then only an entry of that owner named like the repo its
/// stored `origin` names, since its `name` is its folder's. A LocalOnly row
/// is never matched by name: that is how a remote-less checkout picked up a
/// stranger's CI on the first poll.
pub(crate) fn carries(repo: &RepoStatus, entry: &github::GithubRepo) -> bool {
    if repo.location != RepoLocation::LocalOnly {
        return names_remote(repo.owner.as_deref(), &repo.name, entry);
    }
    let (Some(owner), Some(url)) = (repo.owner.as_deref(), repo.remote_url.as_deref()) else {
        return false;
    };
    let Some(origin) = model::RemoteRef::parse(url) else {
        return false;
    };
    names_remote(Some(owner), &origin.name, entry)
}

pub(crate) fn remote_key(owner: Option<&str>, name: &str) -> RemoteKey {
    model::identity_key(owner, name)
}

/// The D4 status line for the first ambiguous clone by path, or None. The
/// count is the owners the full dataset has for the name, since that is what
/// made the folder name ambiguous.
pub(super) fn ambiguity_line(
    ambiguities: &[discovery::Ambiguity],
    dataset: &[github::GithubRepo],
) -> Option<String> {
    let first = ambiguities.iter().min()?;
    let owners: HashSet<String> = dataset
        .iter()
        .filter(|r| r.name.eq_ignore_ascii_case(&first.name))
        .filter_map(|r| r.owner.as_deref().map(str::to_lowercase))
        .collect();
    Some(format!(
        "{}: {} matches {} GitHub repos, add an origin remote",
        first.name,
        tilde_display(&first.path),
        owners.len()
    ))
}

/// Make each LocalOnly row the carrier of the repo its stored `origin` names,
/// when that repo is in the full dataset and no row carries it yet (`carried`
/// holds the keys already carried, and gains each one claimed here). The
/// carrier is marked by its `owner` and takes the repo's GitHub fields now,
/// so they show from the first frame (D6, rule 2 of section 3.5 of
/// `docs/repository-identity-design.md`). Every other LocalOnly row keeps no
/// owner and no GitHub fields.
///
/// The URL is the one `git remote get-url` returned, which applies
/// `insteadOf` and, in a linked worktree, names the main clone's origin, so
/// a checkout passes 1 and 2 could not match can still carry here. Several
/// rows naming one repo are tried in discovery's primary-checkout order
/// (`discovery::CheckoutRank`), each as a match through its exact origin.
///
/// The root index is that of the first scan root holding the path, the same
/// index pass 1 gives a clone two overlapping roots both reach, since the
/// lower-ranked offer wins there. Pass 3 walks only the scan roots, so a row
/// always lies under one; `usize::MAX` is a fallback that cannot occur.
pub(crate) fn mark_local_carriers(
    rows: &mut [(&Path, RepoStatus)],
    dataset: &[github::GithubRepo],
    scan_roots: &[PathBuf],
    carried: &mut HashSet<RemoteKey>,
) {
    let mut order: Vec<(discovery::CheckoutRank, usize, &github::GithubRepo)> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, (path, repo))| {
            let origin = model::RemoteRef::parse(repo.remote_url.as_deref()?)?;
            let entry = dataset.iter().find(|r| {
                r.owner.is_some() && names_remote(Some(&origin.owner), &origin.name, r)
            })?;
            let root = scan_roots
                .iter()
                .position(|root| path.starts_with(root))
                .unwrap_or(usize::MAX);
            let rank = discovery::CheckoutRank::exact_origin(&origin, path, root);
            Some((rank, i, entry))
        })
        .collect();
    order.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, i, entry) in order {
        if !carried.insert(remote_key(entry.owner.as_deref(), &entry.name)) {
            continue;
        }
        let repo = &mut rows[i].1;
        repo.owner = entry.owner.clone();
        repo.apply_github(entry);
    }
}
