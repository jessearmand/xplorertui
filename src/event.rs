use color_eyre::eyre::OptionExt;
use crossterm::event::Event as CrosstermEvent;
use futures::{FutureExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::api::types::{ListResponse, SingleResponse, Tweet, User};

/// The frequency at which tick events are emitted.
const TICK_FPS: f64 = 30.0;

/// Representation of all possible events.
#[derive(Clone, Debug)]
pub enum Event {
    /// An event that is emitted on a regular schedule.
    Tick,
    /// Crossterm events from the terminal.
    Crossterm(CrosstermEvent),
    /// Application-level events.
    App(Box<AppEvent>),
}

/// Application events for navigation, API requests, and API responses.
#[derive(Clone, Debug)]
pub enum AppEvent {
    // -- Navigation --
    Quit,
    PushView(ViewKind),
    PopView,
    SwitchView(ViewKind),

    // -- API request triggers (sent from key handlers) --
    FetchHomeTimeline {
        pagination_token: Option<String>,
    },
    FetchUserTimeline {
        user_id: String,
        pagination_token: Option<String>,
    },
    FetchTweet {
        tweet_id: String,
    },
    FetchThread {
        conversation_id: String,
        pagination_token: Option<String>,
    },
    FetchUser {
        username: String,
    },
    FetchSearch {
        query: String,
        pagination_token: Option<String>,
    },
    FetchMentions {
        pagination_token: Option<String>,
    },
    FetchBookmarks {
        pagination_token: Option<String>,
    },
    FetchFollowers {
        user_id: String,
        pagination_token: Option<String>,
    },
    FetchFollowing {
        user_id: String,
        pagination_token: Option<String>,
    },

    // -- API response events (sent from async tasks back to the event loop) --
    HomeTimelineLoaded(ApiResult<ListResponse<Tweet>>),
    UserTimelineLoaded {
        user_id: String,
        result: ApiResult<ListResponse<Tweet>>,
    },
    TweetLoaded(Box<ApiResult<SingleResponse<Tweet>>>),
    ThreadLoaded {
        conversation_id: String,
        result: ApiResult<ListResponse<Tweet>>,
    },
    UserLoaded(ApiResult<SingleResponse<User>>),
    SearchLoaded {
        query: String,
        result: ApiResult<ListResponse<Tweet>>,
    },
    MentionsLoaded(ApiResult<ListResponse<Tweet>>),
    BookmarksLoaded(ApiResult<ListResponse<Tweet>>),
    FollowersLoaded {
        user_id: String,
        result: ApiResult<ListResponse<User>>,
    },
    FollowingLoaded {
        user_id: String,
        result: ApiResult<ListResponse<User>>,
    },

    // -- Auth --
    AuthCompleted(Result<String, String>),
}

/// API result type using `Arc<String>` so errors are `Clone`.
pub type ApiResult<T> = Result<T, Arc<String>>;

/// Identifies a view for the view-stack navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewKind {
    Home,
    UserTimeline(String), // user_id
    Thread(String),       // tweet_id or conversation_id
    UserProfile(String),  // username
    Search,
    Mentions,
    Bookmarks,
    Help,
}

/// Terminal event handler.
///
/// Spawns a background task that emits tick and crossterm events, and exposes
/// an unbounded channel for application events.
#[derive(Debug)]
pub struct EventHandler {
    /// Event sender channel.
    sender: mpsc::UnboundedSender<Event>,
    /// Event receiver channel.
    receiver: mpsc::UnboundedReceiver<Event>,
}

impl Default for EventHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHandler {
    /// Constructs a new instance of [`EventHandler`] and spawns the event task.
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let actor = EventTask::new(sender.clone());
        tokio::spawn(async { actor.run().await });
        Self { sender, receiver }
    }

    /// Receives the next event, blocking until one is available.
    pub async fn next(&mut self) -> color_eyre::Result<Event> {
        self.receiver
            .recv()
            .await
            .ok_or_eyre("Failed to receive event")
    }

    /// Queue an app event to be processed by the event loop.
    pub fn send(&self, app_event: AppEvent) {
        let _ = self.sender.send(Event::App(Box::new(app_event)));
    }

    /// Clone the underlying sender for use in spawned async tasks.
    pub fn sender(&self) -> mpsc::UnboundedSender<Event> {
        self.sender.clone()
    }
}

/// Background task that reads crossterm events and emits ticks.
struct EventTask {
    sender: mpsc::UnboundedSender<Event>,
}

impl EventTask {
    fn new(sender: mpsc::UnboundedSender<Event>) -> Self {
        Self { sender }
    }

    async fn run(self) -> color_eyre::Result<()> {
        let tick_rate = Duration::from_secs_f64(1.0 / TICK_FPS);
        let mut reader = crossterm::event::EventStream::new();
        let mut tick = tokio::time::interval(tick_rate);
        loop {
            let tick_delay = tick.tick();
            let crossterm_event = reader.next().fuse();
            tokio::select! {
                _ = self.sender.closed() => {
                    break;
                }
                _ = tick_delay => {
                    self.send(Event::Tick);
                }
                Some(Ok(evt)) = crossterm_event => {
                    self.send(Event::Crossterm(evt));
                }
            };
        }
        Ok(())
    }

    fn send(&self, event: Event) {
        let _ = self.sender.send(event);
    }
}
