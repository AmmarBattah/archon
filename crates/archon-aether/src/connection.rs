//! The machinery under [`crate::client::AetherClient`]: transport, one writer
//! task, one reader task that routes replies and events, and the waiters
//! table a dropped connection fails in one sweep.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use archon_core::error::ArchonError;
use serde_json::Value;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt,
};
use tokio::sync::{Notify, mpsc, oneshot};

use crate::events::{AetherEvent, parse_event};
use crate::wire::{
    ClientFrame, Command, ErrorReply, MAX_FRAME_BYTES, RawReply, ReplyResult, ServerFrame,
    Subscribed,
};

pub(crate) type BoxRead = Box<dyn AsyncRead + Send + Unpin>;
pub(crate) type BoxWrite = Box<dyn AsyncWrite + Send + Unpin>;

/// Lines queued for the writer; small, since each request awaits its reply.
pub(crate) const WRITE_QUEUE: usize = 64;
const CLOSED_WITHOUT_REASON: &str = "aetherd closed the connection";

/// What a waiting request receives: a transport failure (outer) or the
/// daemon's own answer, which may be a refusal (inner).
pub(crate) type Answer = Result<Result<RawReply, ErrorReply>, ArchonError>;

/// One subscription's channel and why it ended. The reason is set before the
/// sender drops, so the receiver drains accepted events, then learns why.
#[derive(Debug)]
pub(crate) struct EventSink {
    pub events: mpsc::Sender<AetherEvent>,
    pub closed: Arc<OnceLock<String>>,
    /// Highest seq delivered; a subscription never goes backwards.
    last_seq: u64,
}

impl EventSink {
    pub fn new(events: mpsc::Sender<AetherEvent>, closed: Arc<OnceLock<String>>) -> Self {
        Self {
            events,
            closed,
            last_seq: 0,
        }
    }

    fn close(self, reason: String) {
        // First reason wins: a later one (say, the disconnect after an
        // overflow) would hide what actually ended the stream.
        let _already_closed = self.closed.set(reason);
    }
}

#[derive(Debug)]
enum Pending {
    Reply(oneshot::Sender<Answer>),
    Subscribe(oneshot::Sender<Answer>, EventSink),
}

#[derive(Debug, Default)]
struct Waiters {
    /// Set once, when the connection is known dead; no request waits after.
    dead: Option<String>,
    pending: HashMap<u64, Pending>,
}

/// State shared by the client handle, the reader and the keepalive.
#[derive(Debug)]
pub(crate) struct Shared {
    waiters: Mutex<Waiters>,
    next_req: AtomicU64,
    outgoing: mpsc::Sender<String>,
    request_timeout: Duration,
    /// Signalled when a subscription's consumer goes away, so the reader
    /// releases it without waiting for its next event.
    released: Arc<Notify>,
}

impl Shared {
    pub fn new(outgoing: mpsc::Sender<String>, request_timeout: Duration) -> Self {
        assert!(
            !request_timeout.is_zero(),
            "a zero request timeout fails every request"
        );
        Self {
            waiters: Mutex::new(Waiters::default()),
            next_req: AtomicU64::new(1),
            outgoing,
            request_timeout,
            released: Arc::new(Notify::new()),
        }
    }

    /// What a dropped [`crate::client::Subscription`] signals.
    pub fn released(&self) -> Arc<Notify> {
        self.released.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Waiters> {
        // A panic while holding this lock leaves only a map of senders; the
        // data is still coherent, so recover it rather than cascade the panic.
        self.waiters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn disconnect_reason(&self) -> Option<String> {
        self.lock().dead.clone()
    }

    /// Send one command and wait for its answer, at most `request_timeout`
    /// in all. `sink` is set only for `subscribe`, whose reply registers the stream.
    pub async fn call(&self, command: &Command, sink: Option<EventSink>) -> Answer {
        let req = self.next_req.fetch_add(1, Ordering::Relaxed);
        assert!(req > 0, "request id 0 is reserved for daemon notices");
        let line = encode_frame(req, command)?;
        let (tx, rx) = oneshot::channel();
        self.register(req, tx, sink)?;
        // Unregisters on every exit, including a caller dropping this future.
        let _registered = Registered { shared: self, req };
        // Queueing counts against the timeout: a daemon that stopped reading
        // stalls the writer, and then the queue.
        let exchange = async {
            if self.outgoing.send(line).await.is_err() {
                return Err(self.dead_error());
            }
            rx.await.unwrap_or_else(|_dropped| Err(self.dead_error()))
        };
        match tokio::time::timeout(self.request_timeout, exchange).await {
            Ok(answer) => answer,
            Err(_elapsed) => Err(ArchonError::Unavailable(format!(
                "aetherd did not answer `{}` within {}s",
                command.name(),
                self.request_timeout.as_secs_f64()
            ))),
        }
    }

    fn register(
        &self,
        req: u64,
        tx: oneshot::Sender<Answer>,
        sink: Option<EventSink>,
    ) -> Result<(), ArchonError> {
        let mut waiters = self.lock();
        if let Some(reason) = &waiters.dead {
            return Err(ArchonError::Unavailable(reason.clone()));
        }
        let pending = match sink {
            Some(sink) => Pending::Subscribe(tx, sink),
            None => Pending::Reply(tx),
        };
        let previous = waiters.pending.insert(req, pending);
        assert!(previous.is_none(), "request id {req} was issued twice");
        Ok(())
    }

    fn forget(&self, req: u64) {
        self.lock().pending.remove(&req);
    }

    fn take(&self, req: u64) -> Option<Pending> {
        self.lock().pending.remove(&req)
    }

    fn dead_error(&self) -> ArchonError {
        let reason = self.disconnect_reason();
        ArchonError::Unavailable(reason.unwrap_or_else(|| CLOSED_WITHOUT_REASON.to_owned()))
    }

    /// Mark the connection dead and fail every waiter with `reason`.
    /// Idempotent: the first reason is the one everybody sees.
    pub fn fail_all(&self, reason: &str) {
        let drained: Vec<Pending> = {
            let mut waiters = self.lock();
            if waiters.dead.is_none() {
                waiters.dead = Some(reason.to_owned());
            }
            waiters
                .pending
                .drain()
                .map(|(_, pending)| pending)
                .collect()
        };
        let reason = self
            .disconnect_reason()
            .unwrap_or_else(|| reason.to_owned());
        for pending in drained {
            let tx = match pending {
                Pending::Reply(tx) => tx,
                Pending::Subscribe(tx, sink) => {
                    sink.close(reason.clone());
                    tx
                }
            };
            let _caller_gone = tx.send(Err(ArchonError::Unavailable(reason.clone())));
        }
    }

    /// Queue a command nobody waits for (the reader's `unsubscribe` for a stream
    /// it closed); its reply arrives for an unknown id and is dropped.
    fn fire_and_forget(&self, command: &Command) {
        let req = self.next_req.fetch_add(1, Ordering::Relaxed);
        let line = match encode_frame(req, command) {
            Ok(line) => line,
            Err(error) => {
                tracing::warn!(cmd = %command.name(), %error, "could not encode a courtesy command");
                return;
            }
        };
        match self.outgoing.try_send(line) {
            Ok(()) => {}
            // The reader never waits on the writer, but a dropped unsubscribe
            // would leave Aether streaming into the void; queue it behind.
            Err(mpsc::error::TrySendError::Full(line)) => {
                let outgoing = self.outgoing.clone();
                tokio::spawn(async move {
                    let _connection_gone = outgoing.send(line).await;
                });
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::debug!(cmd = %command.name(), "connection closed; nothing to tell aetherd");
            }
        }
    }
}

/// A waiter-table entry owned by one in-flight [`Shared::call`]. Removing an
/// entry the reader already took is a no-op.
struct Registered<'a> {
    shared: &'a Shared,
    req: u64,
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        self.shared.forget(self.req);
    }
}

fn encode_frame(req: u64, command: &Command) -> Result<String, ArchonError> {
    let mut line = serde_json::to_string(&ClientFrame { req, command })?;
    if line.len() >= MAX_FRAME_BYTES {
        return Err(ArchonError::Invalid(format!(
            "`{}` is {} bytes, over Aether's {MAX_FRAME_BYTES}-byte frame limit",
            command.name(),
            line.len()
        )));
    }
    debug_assert!(!line.contains('\n'), "serde_json never emits raw newlines");
    line.push('\n');
    Ok(line)
}

/// Writes queued lines in order. Exits when every sender is gone or the
/// socket fails, failing all waiters in the latter case.
pub(crate) async fn run_writer(
    mut write: BoxWrite,
    mut lines: mpsc::Receiver<String>,
    shared: Arc<Shared>,
) {
    while let Some(line) = lines.recv().await {
        let written = async {
            write.write_all(line.as_bytes()).await?;
            write.flush().await
        };
        if let Err(error) = written.await {
            shared.fail_all(&format!("writing to aetherd failed: {error}"));
            return;
        }
    }
}

/// Read one newline-terminated frame into `buf`. `Ok(false)` is a clean EOF
/// between frames; a frame over [`MAX_FRAME_BYTES`] is an error.
pub(crate) async fn read_frame<R>(reader: &mut R, buf: &mut Vec<u8>) -> std::io::Result<bool>
where
    R: AsyncBufRead + Unpin,
{
    buf.clear();
    let limit = MAX_FRAME_BYTES as u64 + 1;
    let read = (&mut *reader).take(limit).read_until(b'\n', buf).await?;
    if read == 0 {
        return Ok(false);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        return Ok(true);
    }
    let error = match buf.len() > MAX_FRAME_BYTES {
        true => format!("frame exceeds the {MAX_FRAME_BYTES}-byte limit"),
        false => "connection closed in the middle of a frame".to_owned(),
    };
    Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

/// Early events for a subscription whose `subscribed` reply has not arrived
/// yet: Aether starts replaying before it answers. On overflow the first
/// `capacity` are kept, so a resubscribe from the cursor always progresses.
#[derive(Debug, Default)]
struct EarlyEvents {
    events: Vec<Value>,
    overflowed: bool,
}

/// The reader task's routing state. Owned by that task alone, so
/// registering a subscription and routing its next event cannot race.
#[derive(Debug)]
pub(crate) struct Router {
    shared: Arc<Shared>,
    subs: HashMap<u64, EventSink>,
    early: HashMap<u64, EarlyEvents>,
    /// Highest subscription id registered; Aether numbers them upward, so an
    /// event at or below this for an unknown id belongs to a closed stream.
    highest_sub: u64,
    capacity: usize,
    /// The id-0 notice: the daemon saying why it is about to hang up.
    notice: Option<String>,
}

impl Router {
    pub fn new(shared: Arc<Shared>, capacity: usize) -> Self {
        assert!(
            capacity > 0,
            "a zero-capacity subscription can never deliver"
        );
        Self {
            shared,
            subs: HashMap::new(),
            early: HashMap::new(),
            highest_sub: 0,
            capacity,
            notice: None,
        }
    }

    /// Route frames until the connection ends, then fail everything.
    pub async fn run(mut self, read: BoxRead) {
        let mut reader = tokio::io::BufReader::new(read);
        let mut buf = Vec::new();
        let reason = loop {
            match self.next_frame(&mut reader, &mut buf).await {
                Ok(true) => {}
                Ok(false) => break None,
                Err(error) => break Some(format!("reading from aetherd failed: {error}")),
            }
            if let Err(error) = self.route(&buf) {
                break Some(error);
            }
        };
        let reason = self.notice.take().or(reason);
        self.shutdown(reason.as_deref().unwrap_or(CLOSED_WITHOUT_REASON));
    }

    /// Read the next frame, releasing dropped subscriptions while it waits.
    /// The read itself is never cancelled, so a frame is never torn.
    async fn next_frame<R>(&mut self, reader: &mut R, buf: &mut Vec<u8>) -> std::io::Result<bool>
    where
        R: AsyncBufRead + Unpin,
    {
        let released = self.shared.released();
        let read = read_frame(reader, buf);
        tokio::pin!(read);
        loop {
            tokio::select! {
                frame = &mut read => return frame,
                () = released.notified() => self.release_dropped(),
            }
        }
    }

    /// Forget every subscription whose consumer is gone and tell Aether.
    fn release_dropped(&mut self) {
        let dropped: Vec<u64> = self
            .subs
            .iter()
            .filter(|(_, sink)| sink.events.is_closed())
            .map(|(sub, _)| *sub)
            .collect();
        for sub in dropped {
            self.release(sub);
        }
    }

    /// Nobody listens to `sub` any more, so there is nobody to tell why.
    fn release(&mut self, sub: u64) {
        let removed = self.subs.remove(&sub);
        assert!(removed.is_some(), "released unknown subscription {sub}");
        tracing::debug!(sub, "subscription dropped by its consumer");
        self.shared
            .fire_and_forget(&Command::Unsubscribe { id: sub });
    }

    fn shutdown(&mut self, reason: &str) {
        self.shared.fail_all(reason);
        let reason = self
            .shared
            .disconnect_reason()
            .unwrap_or_else(|| reason.to_owned());
        for (_, sink) in self.subs.drain() {
            sink.close(reason.clone());
        }
        self.early.clear();
    }

    /// Handle one frame. `Err` is fatal to the connection: a frame Archon
    /// cannot decode means it can no longer trust which reply is which.
    fn route(&mut self, line: &[u8]) -> Result<(), String> {
        let frame: ServerFrame = serde_json::from_slice(line).map_err(|error| {
            let shown = String::from_utf8_lossy(&line[..line.len().min(200)]).into_owned();
            format!("undecodable frame from aetherd ({error}): {shown}")
        })?;
        match frame {
            ServerFrame::Reply { id: 0, result } => self.notice = Some(notice_text(result)),
            ServerFrame::Reply { id, result } => self.answer(id, result),
            ServerFrame::Event { sub, event } => self.event(sub, event),
            // Archon subscribes with `deltas: false`; a stray delta is noise.
            ServerFrame::Delta { sub } => tracing::debug!(sub, "ignored a delta frame"),
        }
        Ok(())
    }

    fn answer(&mut self, id: u64, result: ReplyResult) {
        let result = match result {
            ReplyResult::Ok(raw) => Ok(raw),
            ReplyResult::Err(refusal) => Err(refusal),
        };
        match self.shared.take(id) {
            None => self.orphan(id, &result),
            Some(Pending::Reply(tx)) => {
                let _caller_gone = tx.send(Ok(result));
            }
            Some(Pending::Subscribe(tx, sink)) => {
                let answer = match &result {
                    Ok(raw) => self.register(raw, sink).map(|()| result),
                    Err(_) => Ok(result),
                };
                let _caller_gone = tx.send(answer);
            }
        }
    }

    /// A reply nobody waits for: its request timed out. A late `subscribed`
    /// would otherwise stream into the void, so it is unsubscribed.
    fn orphan(&mut self, id: u64, result: &Result<RawReply, ErrorReply>) {
        tracing::debug!(id, "reply to a request nobody waits for (timed out?)");
        let Ok(raw) = result else {
            return;
        };
        let Ok(subscribed) = serde_json::from_value::<Subscribed>(raw.data.clone()) else {
            return;
        };
        if raw.reply == "subscribed" && subscribed.id > self.highest_sub {
            self.highest_sub = subscribed.id;
            self.early.retain(|sub, _| *sub > subscribed.id);
            self.shared
                .fire_and_forget(&Command::Unsubscribe { id: subscribed.id });
        }
    }

    /// Install a subscription the moment its reply is read, then deliver the
    /// events that raced ahead of the reply.
    fn register(&mut self, raw: &RawReply, mut sink: EventSink) -> Result<(), ArchonError> {
        if raw.reply != "subscribed" {
            return Err(ArchonError::Protocol(format!(
                "expected a `subscribed` reply, got `{}`",
                raw.reply
            )));
        }
        let subscribed: Subscribed = serde_json::from_value(raw.data.clone())?;
        if subscribed.id <= self.highest_sub || self.subs.contains_key(&subscribed.id) {
            return Err(ArchonError::Protocol(format!(
                "aetherd reused subscription id {}",
                subscribed.id
            )));
        }
        self.highest_sub = subscribed.id;
        sink.last_seq = subscribed.last_seq;
        let early = self.early.remove(&subscribed.id).unwrap_or_default();
        let abandoned = sink.events.is_closed();
        self.subs.insert(subscribed.id, sink);
        if abandoned {
            // The subscriber was cancelled as this reply arrived, and its
            // release signal was spent before the stream was installed.
            self.release(subscribed.id);
            return Ok(());
        }
        assert!(
            early.events.len() <= self.capacity,
            "early events are capped at the channel capacity, so all fit"
        );
        for event in early.events {
            self.deliver(subscribed.id, event);
        }
        if early.overflowed {
            let reason = self.overflow_reason(subscribed.id);
            self.close(subscribed.id, reason);
        }
        Ok(())
    }

    fn event(&mut self, sub: u64, event: Value) {
        if self.subs.contains_key(&sub) {
            self.deliver(sub, event);
            return;
        }
        if sub <= self.highest_sub {
            // A stream Archon already closed; the consumer has its error.
            return;
        }
        let early = self.early.entry(sub).or_default();
        match early.events.len() < self.capacity {
            true => early.events.push(event),
            false => early.overflowed = true,
        }
    }

    fn deliver(&mut self, sub: u64, raw: Value) {
        let Some(sink) = self.subs.get_mut(&sub) else {
            return;
        };
        let event = match parse_event(raw) {
            Ok(event) => event,
            Err(error) => {
                self.close(
                    sub,
                    format!("subscription {sub} got a malformed event: {error}"),
                );
                return;
            }
        };
        if event.seq <= sink.last_seq {
            let reason = format!(
                "subscription {sub} went backwards: seq {} after {}",
                event.seq, sink.last_seq
            );
            self.close(sub, reason);
            return;
        }
        let seq = event.seq;
        match sink.events.try_send(event) {
            Ok(()) => sink.last_seq = seq,
            Err(mpsc::error::TrySendError::Full(_)) => {
                let reason = self.overflow_reason(sub);
                self.close(sub, reason);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => self.release(sub),
        }
    }

    fn overflow_reason(&self, sub: u64) -> String {
        format!(
            "subscription {sub} overflowed its {}-event buffer; resubscribe from the last \
             handled seq",
            self.capacity
        )
    }

    /// End one stream: record why, drop its sender, and tell Aether to stop.
    fn close(&mut self, sub: u64, reason: String) {
        let Some(sink) = self.subs.remove(&sub) else {
            return;
        };
        tracing::warn!(sub, %reason, "closing an aether subscription");
        sink.close(reason);
        self.shared
            .fire_and_forget(&Command::Unsubscribe { id: sub });
    }
}

fn notice_text(result: ReplyResult) -> String {
    match result {
        ReplyResult::Err(refusal) => format!("aetherd closed the connection: {refusal}"),
        ReplyResult::Ok(raw) => format!("aetherd sent an unexpected `{}` notice", raw.reply),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frames_split_on_newlines_and_eof_is_clean_only_between_frames() {
        let mut input: &[u8] = b"{\"a\":1}\n{\"b\":2}\n{\"c\"";
        let mut buf = Vec::new();
        assert!(read_frame(&mut input, &mut buf).await.unwrap());
        assert_eq!(buf, b"{\"a\":1}");
        assert!(read_frame(&mut input, &mut buf).await.unwrap());
        assert_eq!(buf, b"{\"b\":2}");
        let torn = read_frame(&mut input, &mut buf).await.unwrap_err();
        assert!(torn.to_string().contains("middle of a frame"));
        let mut empty: &[u8] = b"";
        assert!(!read_frame(&mut empty, &mut buf).await.unwrap());
    }

    #[tokio::test]
    async fn an_oversized_frame_is_refused_not_buffered_forever() {
        let big = vec![b'x'; MAX_FRAME_BYTES + 10];
        let mut input: &[u8] = &big;
        let mut buf = Vec::new();
        let error = read_frame(&mut input, &mut buf).await.unwrap_err();
        assert!(error.to_string().contains("limit"));
        assert!(buf.len() <= MAX_FRAME_BYTES + 1);
    }

    #[tokio::test]
    async fn a_cancelled_request_leaves_no_waiter_behind() {
        let (tx, mut rx) = mpsc::channel(1);
        let shared = Shared::new(tx, Duration::from_secs(60));
        let abandoned =
            tokio::time::timeout(Duration::from_millis(20), shared.call(&Command::Ping, None));
        assert!(
            abandoned.await.is_err(),
            "nobody answers, so the caller gives up"
        );
        assert!(rx.recv().await.is_some(), "the ping was sent");
        assert!(
            shared.lock().pending.is_empty(),
            "the dropped call unregistered"
        );
    }

    #[tokio::test]
    async fn a_courtesy_command_waits_for_room_instead_of_being_dropped() {
        let (tx, mut rx) = mpsc::channel(1);
        let shared = Shared::new(tx, Duration::from_secs(1));
        shared.outgoing.try_send("busy\n".to_owned()).unwrap();
        shared.fire_and_forget(&Command::Unsubscribe { id: 3 });
        assert_eq!(rx.recv().await.unwrap(), "busy\n");
        let queued = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("the unsubscribe was queued, not dropped")
            .unwrap();
        let value: Value = serde_json::from_str(queued.trim_end()).unwrap();
        assert_eq!(
            (&value["cmd"], &value["id"]),
            (&"unsubscribe".into(), &3.into())
        );
    }

    #[tokio::test]
    async fn a_reply_for_a_subscriber_already_gone_is_released_at_once() {
        let (tx, mut rx) = mpsc::channel(4);
        let shared = Arc::new(Shared::new(tx, Duration::from_secs(1)));
        let mut router = Router::new(shared.clone(), 4);
        let (events_tx, events) = mpsc::channel(4);
        drop(events);
        let (reply_tx, _reply_rx) = oneshot::channel();
        let sink = EventSink::new(events_tx, Arc::default());
        shared.register(1, reply_tx, Some(sink)).unwrap();
        let reply = br#"{"frame":"reply","id":1,"result":{"Ok":{"reply":"subscribed","data":{"id":5,"last_seq":0}}}}"#;
        router.route(reply).unwrap();
        assert!(
            router.subs.is_empty(),
            "nobody reads the stream, so none is kept"
        );
        let queued = rx.recv().await.unwrap();
        let value: Value = serde_json::from_str(queued.trim_end()).unwrap();
        assert_eq!(
            (&value["cmd"], &value["id"]),
            (&"unsubscribe".into(), &5.into())
        );
    }

    #[test]
    fn frames_carry_req_and_cmd_and_end_in_one_newline() {
        let line = encode_frame(7, &Command::Ping).unwrap();
        assert_eq!(line, "{\"req\":7,\"cmd\":\"ping\"}\n");
        let value: Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(value["req"], 7);
    }
}
