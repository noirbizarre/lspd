//! `lspd daemon` — plumbing, not normally invoked directly.
//!
//! Other commands start a daemon on demand via
//! `lspd::client::discover_or_start`; this subcommand is what that spawn
//! call actually execs, and is also the form to run manually (in the
//! foreground) while debugging the daemon itself.

use super::Context;

pub async fn serve(ctx: &Context) -> lspd::Result<()> {
    lspd::daemon::serve(ctx.root.clone()).await
}
