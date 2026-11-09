//! The engine (A3 and A5 in `docs/architecture.md` section 7): the view
//! assembly both front ends call, the TUI in `main.rs` and print mode in
//! `report`. No terminal crate and nothing from `main`, `app`, `ui` or
//! `report`; progress leaves through a `Sink` the front end supplies.

mod assembly;
mod carriers;
mod fetch;
mod load;
mod status;

pub use assembly::ApplyProgress;
pub use fetch::FetchClaims;
pub use load::{DatasetSource, LoadError};

pub(crate) use assembly::{
    FETCH_CONCURRENCY, ViewNotes, activity_position, assemble_view, assemble_view_held,
    probe_concurrency, resolve_cutoff, unclaimed_local,
};
pub(crate) use carriers::{RemoteKey, carries, mark_local_carriers, names_remote, remote_key};
pub(crate) use fetch::fetch_under_claim_capped;
pub(crate) use load::load_repos;
pub(crate) use status::{HISTORY_COUNT, disk_usage_of, tilde_display};

#[cfg(test)]
pub(crate) use assembly::{assemble_view_in, with_coverage_note};
#[cfg(test)]
pub(crate) use fetch::PROGRESS_EVENT_GAP;

/// Where the engine sends progress: a front end's event channel, through
/// that front end's `From` impl for its own event type.
pub(crate) trait Sink<T> {
    fn send_event(&self, t: T);
}

impl<T, E: From<T>> Sink<T> for tokio::sync::mpsc::UnboundedSender<E> {
    fn send_event(&self, t: T) {
        let _ = self.send(t.into());
    }
}

#[cfg(test)]
mod tests;
