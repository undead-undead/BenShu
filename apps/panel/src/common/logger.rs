use std::fmt::Write as _;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::Subscriber;
use tracing_subscriber::Layer;

/// A simple tracing layer that broadcasts log lines to a channel.
pub struct BroadcastLayer {
    sender: broadcast::Sender<String>,
}

impl BroadcastLayer {
    pub fn new(sender: broadcast::Sender<String>) -> Self {
        Self { sender }
    }
}

impl<S> Layer<S> for BroadcastLayer
where
    S: Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut buf = String::new();

        // 1. Add Level Prefix
        let level = *event.metadata().level();
        let _ = std::fmt::Write::write_fmt(&mut buf, format_args!("[{}] ", level));

        // 2. Add Target (module)
        let _ =
            std::fmt::Write::write_fmt(&mut buf, format_args!("<{}> ", event.metadata().target()));

        let mut visitor = LogVisitor(&mut buf);
        event.record(&mut visitor);

        // Push and ignore errors (if no subscribers)
        let _ = self.sender.send(buf);
    }
}

struct LogVisitor<'a>(&'a mut String);

impl<'a> tracing::field::Visit for LogVisitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            use std::fmt::Write;
            let _ = write!(self.0, "{:?}", value);
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0.push_str(value);
        }
    }
}
