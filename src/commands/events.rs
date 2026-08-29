//! `projectd events` — subscribes to the shared filesystem event stream.
//!
//! Requires a daemon (starting one if needed): the whole point of the shared
//! watcher is that this command does not run its own.

use projectd::client;
use projectd::daemon::protocol::{Request, Response};

use super::Context;

pub async fn run(ctx: &Context, paths: Vec<String>) -> projectd::Result<()> {
    let mut conn = client::discover_or_start(&ctx.root).await?;
    client::request(&mut conn, Request::Subscribe { paths })
        .await
        .and_then(|first| print_event(ctx, first))?;

    // `Subscribe` switches the daemon side into push-only mode: it keeps
    // sending `Event` frames on its own until this connection closes, so
    // this reads directly off the connection rather than issuing further
    // requests (see `client::read_response`'s docs).
    loop {
        let response = client::read_response(&mut conn).await?;
        print_event(ctx, response)?;
    }
}

fn print_event(ctx: &Context, response: Response) -> projectd::Result<()> {
    match response {
        Response::Event(event) => {
            if ctx.json {
                projectd::output::print_ok(&event);
            } else {
                println!("{event:?}");
            }
            Ok(())
        }
        other => Err(client::unexpected_response(other)),
    }
}
