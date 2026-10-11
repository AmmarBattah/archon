//! `SubscribeFeed`: the backlog after the client's cursor, then live items,
//! in `seq` order without gaps or repeats. The store is the source of truth;
//! the broadcast channel only says that something new was appended.

use std::sync::Arc;

use archon_core::error::ArchonError;
use archon_core::notify::FeedItem;
use archon_core::protocol::{ErrorCode, ErrorReply, Reply, ServerFrame};
use tokio::sync::broadcast::error::RecvError;

use super::{Outbox, reply, send};
use crate::context::DaemonContext;

/// Feed items read from the store per query while catching up.
const CATCH_UP_BATCH: usize = 500;

pub(super) struct Subscription {
    pub id: u64,
    /// The `SubscribeFeed` request's id, for the `Subscribed` reply.
    pub request: u64,
    pub after: Option<u64>,
    pub verbose: bool,
}

/// Runs until the connection closes; a store failure ends the subscription
/// with an id-0 error so the client can resubscribe from its cursor.
pub(super) async fn run(ctx: Arc<DaemonContext>, sub: Subscription, outbox: Outbox) {
    if let Err(error) = stream(&ctx, &sub, &outbox).await {
        tracing::warn!(sub = sub.id, %error, "feed subscription ended");
        let frame = ServerFrame::Reply {
            id: 0,
            result: Err(ErrorReply {
                code: ErrorCode::Internal,
                message: format!("feed subscription {} ended: {error}", sub.id),
            }),
        };
        send(&outbox, frame).await;
    }
}

async fn stream(
    ctx: &DaemonContext,
    sub: &Subscription,
    outbox: &Outbox,
) -> Result<(), ArchonError> {
    // Subscribe before reading the store, so nothing appended in between is missed.
    let mut live = ctx.feed.subscribe();
    let last_seq = ctx.store.lock().await.last_feed_seq()?;
    let subscribed = Reply::Subscribed {
        id: sub.id,
        last_seq,
    };
    if !send(outbox, reply(sub.request, Ok(subscribed))).await {
        return Ok(());
    }
    let mut cursor = sub.after.unwrap_or(last_seq).min(last_seq);
    if !catch_up(ctx, sub, outbox, &mut cursor).await? {
        return Ok(());
    }
    loop {
        let received = tokio::select! {
            received = live.recv() => received,
            () = outbox.closed() => return Ok(()),
        };
        let open = match received {
            Ok(item) if item.seq <= cursor => true,
            Ok(item) if item.seq == cursor + 1 => deliver(sub, outbox, &mut cursor, item).await,
            // A gap (appends raced) or a lagging receiver: the store has it all.
            Ok(_) | Err(RecvError::Lagged(_)) => catch_up(ctx, sub, outbox, &mut cursor).await?,
            Err(RecvError::Closed) => return Ok(()),
        };
        if !open {
            return Ok(());
        }
    }
}

/// Send every stored item after `cursor`. `false` once the client is gone.
async fn catch_up(
    ctx: &DaemonContext,
    sub: &Subscription,
    outbox: &Outbox,
    cursor: &mut u64,
) -> Result<bool, ArchonError> {
    loop {
        let batch = ctx.store.lock().await.feed_after(*cursor, CATCH_UP_BATCH)?;
        let done = batch.len() < CATCH_UP_BATCH;
        for item in batch {
            assert!(
                item.seq > *cursor,
                "feed_after returns later items in order"
            );
            if !deliver(sub, outbox, cursor, item).await {
                return Ok(false);
            }
        }
        if done {
            return Ok(true);
        }
    }
}

/// Advance the cursor past `item`, sending it when this client shows it.
async fn deliver(sub: &Subscription, outbox: &Outbox, cursor: &mut u64, item: FeedItem) -> bool {
    debug_assert_eq!(item.seq, *cursor + 1, "the feed is gapless");
    *cursor = item.seq;
    if !item.visible(sub.verbose) {
        return true;
    }
    let frame = ServerFrame::Feed { sub: sub.id, item };
    send(outbox, frame).await
}
