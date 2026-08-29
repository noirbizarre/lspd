//! `projectd daemon` — plumbing, not normally invoked directly.
//!
//! Other commands start a daemon on demand via
//! `projectd::client::discover_or_start`; this subcommand is what that spawn
//! call actually execs, and is also the form to run manually (in the
//! foreground) while debugging the daemon itself.

use super::Context;

pub async fn serve(ctx: &Context) -> projectd::Result<()> {
    projectd::daemon::serve(ctx.root.clone()).await
}
